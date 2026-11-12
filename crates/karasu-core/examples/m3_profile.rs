//! Dependency-free M3 measurement worker. See tools/m3-profile/README.md.

use karasu_core::{
    cancellation::Cancellation,
    geometry::Point3,
    mesh::{
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        TriangleMesh,
    },
    slicing::{
        construct::{
            union_material_rings, ConstructError, ConstructLimits, ConstructResource,
            ConstructedMaterial, MaterialRing,
        },
        engine::{
            collect_layers, CollectionLimits, LayerCollection, LayerConfig, LayerEngine,
            LayerStageLimits,
        },
        grid::BinaryGridSpec,
        regions::{RegionCompleteness, RegionFillRule},
        schedule::{
            build_layer_schedule, LayerSchedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec,
        },
        SectionTransform,
    },
};
use std::{error::Error, hint::black_box, time::Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

enum Workload {
    Layers {
        ribbon: bool,
        represented: bool,
        mesh: TriangleMesh,
        schedule: LayerSchedule,
        regions: usize,
        rings: usize,
        non_intersecting: usize,
    },
    Construct {
        polygons: Vec<Vec<[i64; 2]>>,
        pair_boundary: bool,
        rational: bool,
    },
}
#[allow(clippy::large_enum_variant)] // Avoid adding a synthetic heap allocation to measured work.
enum Output {
    Layers(LayerCollection),
    Construct(ConstructedMaterial),
    Rejection(ConstructError),
}

fn rectangle(x: i32, y: i32, width: i32, height: i32) -> Vec<[i32; 2]> {
    vec![
        [x, y],
        [x + width, y],
        [x + width, y + height],
        [x, y + height],
    ]
}

fn material_rings(polygons: &[Vec<[i64; 2]>]) -> Result<Vec<MaterialRing<'_>>> {
    polygons
        .iter()
        .enumerate()
        .map(|(id, vertices)| {
            Ok(MaterialRing {
                vertices,
                material: u64::try_from(id)?,
                source_path: id,
            })
        })
        .collect()
}

fn ribbon(polygons: &[Vec<[i32; 2]>], extra_faces: usize) -> Result<TriangleMesh> {
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for polygon in polygons {
        let start = u32::try_from(vertices.len())?;
        for &[x, y] in polygon {
            vertices.extend([
                Point3::new(f64::from(x), f64::from(y), -1.0),
                Point3::new(f64::from(x), f64::from(y), 1.0),
            ]);
        }
        for index in 0..polygon.len() {
            let first = start + 2 * u32::try_from(index)?;
            let second = start + 2 * u32::try_from((index + 1) % polygon.len())?;
            triangles.extend([[first, second, second + 1], [first, second + 1, first + 1]]);
        }
    }
    if extra_faces != 0 {
        let start = u32::try_from(vertices.len())?;
        vertices.extend([
            Point3::new(0.0, 0.0, 2.0),
            Point3::new(1.0, 0.0, 2.0),
            Point3::new(0.0, 1.0, 2.0),
        ]);
        // Deliberately retained duplicate faces exercise the full scan above every plane.
        triangles.extend(std::iter::repeat_n(
            [start, start + 1, start + 2],
            extra_faces,
        ));
    }
    Ok(TriangleMesh::try_new(vertices, triangles)?)
}

fn validate_reference_layer(layer: &karasu_core::slicing::engine::RegionLayer) -> Result<()> {
    use karasu_core::slicing::area::{AreaMagnitude, PathAreaOutcome};
    let PathAreaOutcome::Measured {
        magnitude: AreaMagnitude::Finite(area),
        ..
    } = layer.paths()[layer.boundaries()[0].path].area
    else {
        return Err("missing finite reference area".into());
    };
    let ideal = std::f64::consts::PI * (900.0 - (layer.plane().z_mm - 30.0).powi(2));
    require(
        (area - ideal).abs() / ideal < 0.001,
        "reference sphere area envelope exceeded",
    )?;
    Ok(())
}

fn reference_large() -> Result<Workload> {
    let mesh = import_stl_path(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/reference/stl/fx-11-high-complexity.stl"),
        StlOptions::new(StlUnitPolicy::AssumeMillimeters),
    )?
    .mesh;
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(15.0, 15.0, ScheduleEnd::Count(3))?,
        ScheduleLimits {
            max_layers: 3,
            max_payload_bytes: 4096,
        },
    )?;
    Ok(Workload::Layers {
        ribbon: false,
        represented: true,
        mesh,
        schedule,
        regions: 1,
        rings: 1,
        non_intersecting: 0,
    })
}

impl Workload {
    fn new(case: &str) -> Result<Self> {
        let layer = |polygons: Vec<Vec<[i32; 2]>>, regions, rings, extra_faces| -> Result<Self> {
            Ok(Self::Layers {
                ribbon: true,
                represented: false,
                mesh: ribbon(&polygons, extra_faces)?,
                schedule: build_layer_schedule(
                    UniformScheduleSpec::try_new(-0.5, 0.5, ScheduleEnd::Count(3))?,
                    ScheduleLimits {
                        max_layers: 3,
                        max_payload_bytes: 4096,
                    },
                )?,
                regions,
                rings,
                non_intersecting: extra_faces,
            })
        };
        match case {
            "represented-nested" | "represented-many" => {
                let original = if case == "represented-nested" {
                    "strict-nested"
                } else {
                    "strict-many"
                };
                let mut workload = Self::new(original)?;
                if let Self::Layers {
                    represented,
                    schedule,
                    ..
                } = &mut workload
                {
                    *represented = true;
                    *schedule = build_layer_schedule(
                        UniformScheduleSpec::try_new(-0.75, 0.25, ScheduleEnd::Count(7))?,
                        ScheduleLimits {
                            max_layers: 7,
                            max_payload_bytes: 4096,
                        },
                    )?;
                }
                Ok(workload)
            }
            "reference-large" => reference_large(),
            "face-million" => layer(vec![rectangle(0, 0, 4, 4)], 1, 1, 1_000_000),
            "regions-large" => layer(
                (0..1024)
                    .map(|i| rectangle(8 * (i % 32), 8 * (i / 32), 4, 4))
                    .collect(),
                1024,
                1024,
                0,
            ),
            "strict-nested" => {
                let mut polygons: Vec<_> = (0..8)
                    .map(|i| rectangle(2 * i, 2 * i, 64 - 4 * i, 64 - 4 * i))
                    .collect();
                polygons.push(rectangle(100, 0, 4, 4));
                layer(polygons, 5, 9, 0)
            }
            "strict-many" => layer(
                (0..64)
                    .map(|i| rectangle(8 * (i % 8), 8 * (i / 8), 4, 4))
                    .collect(),
                64,
                64,
                0,
            ),
            "face-heavy" => layer(vec![rectangle(0, 0, 4, 4)], 1, 1, 50_000),
            "construct-overlap" | "construct-large" => Ok(Self::Construct {
                polygons: (0..if case == "construct-large" { 64 } else { 24 })
                    .map(|i| {
                        rectangle(i, i, 8, 8)
                            .into_iter()
                            .map(|p| p.map(i64::from))
                            .collect()
                    })
                    .collect(),
                pair_boundary: false,
                rational: false,
            }),
            "construct-rational" | "pair-cap" => Ok(Self::Construct {
                polygons: vec![vec![[0, 0], [6, 0], [0, 6]], vec![[1, -1], [5, 7], [7, -1]]],
                pair_boundary: case == "pair-cap",
                rational: case == "construct-rational",
            }),
            _ => Err("unknown measurement case; see tools/m3-profile/README.md".into()),
        }
    }

    fn run(&self) -> Result<Output> {
        match self {
            Self::Layers {
                mesh,
                schedule,
                represented,
                ..
            } => {
                let stream = LayerEngine::new(
                    mesh,
                    schedule,
                    SectionTransform::Identity,
                    LayerConfig {
                        grid: BinaryGridSpec::try_new([0.0, 0.0], -3)?,
                        fill_rule: RegionFillRule::EvenOddStrict,
                        stages: LayerStageLimits {
                            section: karasu_core::slicing::SectionLimits {
                                max_faces: 2_000_000,
                                ..karasu_core::slicing::SectionLimits::default()
                            },
                            validity: karasu_core::slicing::validity::ValidityLimits {
                                max_pairs: 40_000_000,
                                ..karasu_core::slicing::validity::ValidityLimits::default()
                            },
                            ..LayerStageLimits::default()
                        },
                        max_output_payload_bytes: 64 * 1024 * 1024,
                    },
                    Cancellation::never(),
                    None,
                );
                let stream = if *represented {
                    stream.with_represented_coordinates()
                } else {
                    stream
                };
                Ok(Output::Layers(collect_layers(
                    stream,
                    CollectionLimits {
                        max_layers: u64::try_from(schedule.planes().len())?,
                        max_payload_bytes: 256 * 1024 * 1024,
                    },
                )?))
            }
            Self::Construct {
                polygons,
                pair_boundary,
                ..
            } => {
                let rings = material_rings(polygons)?;
                let limits = ConstructLimits {
                    max_pairs: if *pair_boundary { 14 } else { 1_000_000 },
                    max_work: if polygons.len() > 24 {
                        100_000_000
                    } else {
                        ConstructLimits::default().max_work
                    },
                    ..ConstructLimits::default()
                };
                match union_material_rings(&rings, limits) {
                    Ok(result) => Ok(Output::Construct(result)),
                    Err(error) if *pair_boundary => Ok(Output::Rejection(error)),
                    Err(error) => Err(error.into()),
                }
            }
        }
    }

    fn validate(&self, output: &Output) -> Result<usize> {
        match (self, output) {
            (
                Self::Layers {
                    mesh,
                    schedule,
                    ribbon,
                    represented,
                    regions,
                    rings,
                    non_intersecting,
                },
                Output::Layers(result),
            ) => {
                require(
                    result.layers().len() == schedule.planes().len(),
                    "all scheduled layers required",
                )?;
                for (index, layer) in result.layers().iter().enumerate() {
                    require(
                        layer.plane().index == u64::try_from(index)?,
                        "noncontiguous plane index",
                    )?;
                    require(
                        layer.completeness() == RegionCompleteness::Complete,
                        "unresolved strict layer",
                    )?;
                    require(
                        layer.regions().len() == *regions && layer.boundaries().len() == *rings,
                        "analytic strict region or ring count mismatch",
                    )?;
                    require(
                        layer.raw().summary().source_faces == mesh.triangles().len()
                            && (!ribbon
                                || layer.raw().summary().non_intersecting_faces
                                    == *non_intersecting),
                        "full source-face accounting mismatch",
                    )?;
                    require(
                        !ribbon || layer.raw().events().len() == 8 * rings,
                        "analytic ribbon event count mismatch",
                    )?;
                    if !ribbon {
                        validate_reference_layer(layer)?;
                    }
                    require(
                        layer.plane().z_mm.to_bits() == schedule.planes()[index].z_mm.to_bits(),
                        "analytic plane mismatch",
                    )?;
                }
                require(
                    result
                        .layers()
                        .iter()
                        .all(|layer| layer.uses_represented_coordinates() == *represented),
                    "coordinate mode mismatch",
                )?;
                Ok(result.payload_bytes())
            }
            (
                Self::Construct {
                    pair_boundary: false,
                    rational,
                    ..
                },
                Output::Construct(result),
            ) => validate_construct(result, *rational),
            (
                Self::Construct {
                    pair_boundary: true,
                    polygons,
                    ..
                },
                Output::Rejection(error),
            ) => {
                require(
                    *error
                        == ConstructError::LimitExceeded {
                            resource: ConstructResource::Pairs,
                            limit: 14,
                        },
                    "expected exact pair-cap rejection",
                )?;
                let rings = material_rings(polygons)?;
                // The same six original edges succeed with their exact 6*5/2 pair budget.
                let accepted = union_material_rings(
                    &rings,
                    ConstructLimits {
                        max_pairs: 15,
                        ..ConstructLimits::default()
                    },
                )?;
                require(
                    accepted.summary().pairs == 15 && accepted.regions().len() == 1,
                    "pair-cap boundary acceptance mismatch",
                )?;
                Ok(0)
            }
            _ => Err("unexpected workload output".into()),
        }
    }
}

fn validate_construct(result: &ConstructedMaterial, rational: bool) -> Result<usize> {
    require(
        result.regions().len() == 1 && result.rings().len() == 1,
        "analytic connected union must have one outer and no holes",
    )?;
    require(!result.edges().is_empty(), "empty union boundary")?;
    let mut retained = 0;
    for atom in result.atoms() {
        require(
            atom.boundary.is_some() == (atom.left_material != atom.right_material),
            "atom boundary side mismatch",
        )?;
        retained += usize::from(atom.boundary.is_some());
    }
    require(
        retained == result.edges().len(),
        "boundary atom conservation mismatch",
    )?;
    if rational {
        let noninteger = result.vertices().iter().any(|point| {
            let words = point.denominator();
            let denominator = u128::from(words[0]) | (u128::from(words[1]) << 64);
            [point.numerator_x(), point.numerator_y()]
                .iter()
                .any(|(_, words)| {
                    let numerator = u128::from(words[0]) | (u128::from(words[1]) << 64);
                    numerator % denominator != 0
                })
        });
        require(
            noninteger,
            "rational fixture must construct a noninteger coordinate",
        )?;
    }
    Ok(result.payload_bytes())
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    require(args.len() == 3, "expected timing|memory case repetitions")?;
    let mode = &args[0];
    let case = &args[1];
    require(
        matches!(mode.as_str(), "timing" | "memory"),
        "unknown measurement mode",
    )?;
    let repetitions: usize = args[2].parse()?;
    require(
        (1..=32).contains(&repetitions),
        "repetitions must be in 1..=32",
    )?;
    let workload = Workload::new(case)?;
    if mode == "memory" {
        let output = black_box(workload.run()?);
        let payload = workload.validate(black_box(&output))?;
        println!("validation=PASS case={case} retained_payload_bytes={payload}");
        return Ok(());
    }
    // Independent fixture checks run before timing and again outside every measured interval.
    let initial = workload.run()?;
    let payload = workload.validate(&initial)?;
    drop(initial);
    println!("sample,total_ns,repetitions,retained_payload_bytes_per_operation");
    for sample in 0..13 {
        let mut outputs = Vec::with_capacity(repetitions);
        let start = Instant::now();
        for _ in 0..repetitions {
            outputs.push(black_box(workload.run()?));
        }
        let elapsed = start.elapsed().as_nanos();
        for output in &outputs {
            workload.validate(black_box(output))?;
        }
        if sample >= 3 {
            println!("{},{elapsed},{repetitions},{payload}", sample - 3);
        }
    }
    eprintln!(
        "validation=PASS case={case} warmup_batches=3 timed_batches=10 repetitions={repetitions}"
    );
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("m3-profile: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
