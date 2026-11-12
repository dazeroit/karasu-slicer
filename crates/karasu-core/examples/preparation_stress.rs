//! Bounded eight-support prepared-material stress fixture; timings are observations only.
use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::{
        stl::{import_stl_bytes, StlOptions, StlUnitPolicy},
        TriangleMesh,
    },
    preparation::{
        supports::{
            generate_supports, SupportContact, SupportIntersectionPolicy, SupportLimits,
            SupportPlan, SupportProfile, SupportRecipe, SupportStation, SupportedModel,
        },
        HoleSpec, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding,
    },
    raster::{
        rasterize_layer, Compensation, LcdProfile, Orientation, OutsidePolicy, RasterConfig,
        RasterLimits,
    },
    slicing::{
        construct::ConstructLimits,
        engine::{LayerConfig, LayerStageLimits},
        grid::BinaryGridSpec,
        regions::RegionFillRule,
        schedule::{build_layer_schedule, ScheduleEnd, ScheduleLimits, UniformScheduleSpec},
    },
};
use std::{
    error::Error,
    io::{self, Write},
    path::PathBuf,
    time::Instant,
};
const SOURCE_BYTES: &[u8] = include_bytes!("../../../tools/m55e-validate/source.stl");
const SOURCE_SHA256: [u8; 32] = [
    248, 254, 115, 33, 92, 40, 133, 155, 175, 249, 29, 184, 116, 213, 31, 187, 224, 166, 198, 46,
    242, 6, 168, 14, 181, 146, 61, 247, 190, 200, 187, 198,
];

/// The owned cube translated ten millimeters, hollowed and drained.
pub(crate) fn model_plan() -> PreparationPlan {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: SOURCE_SHA256,
            placement: ObjectTransform::new(
                Vector3::new(0., 0., 10.),
                Rotation3::identity(),
                Vector3::new(1., 1., 1.),
                Point3::new(0., 0., 0.),
            )
            .unwrap(),
        },
        SamplingSpec {
            cell_mm: 0.1,
            distance_error_mm: 0.001,
            max_cells: 1,
        },
        vec![
            Operation {
                id: 1,
                kind: OperationKind::Hollow(HollowSpec {
                    blockers: Vec::new(),
                    thickness_mm: 2.,
                    guarantee: None,
                }),
            },
            Operation {
                id: 2,
                kind: OperationKind::Hole(HoleSpec {
                    entry_mm: [5., 5., 9.],
                    direction: [0., 0., 1.],
                    length_mm: 4.,
                    diameter_mm: 2.,
                }),
            },
        ],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

/// Decode the fixed owned STL source whose digest is recorded above.
pub(crate) fn source() -> TriangleMesh {
    import_stl_bytes(
        SOURCE_BYTES,
        StlOptions::new(StlUnitPolicy::AssumeMillimeters),
    )
    .unwrap()
    .mesh
}
/// Author eight separated rectangular support recipes under explicit limits.
pub(crate) fn plan(
    limits: SupportLimits,
) -> Result<SupportPlan, karasu_core::preparation::supports::SupportError> {
    let model = model_plan();
    let mut recipes = Vec::new();
    for y in [3., 9.] {
        for x in [9., 12., 15., 18.] {
            let stations = [(0., 2.), (1., 2.), (9., 1.), (10.3, 0.4)]
                .into_iter()
                .map(|(z, size)| SupportStation {
                    center_mm: [x, y, z],
                    rotation: Rotation3::identity(),
                    profile: SupportProfile::Rectangle {
                        size_mm: [size, size],
                    },
                })
                .collect();
            recipes.push(SupportRecipe {
                id: u64::try_from(recipes.len()).unwrap() + 1,
                surface: model.model_key(),
                contact: SupportContact {
                    source_triangle: 0,
                    position_mm: [x, y, 10.],
                    maximum_penetration_mm: 0.5,
                },
                stations,
            });
        }
    }
    SupportPlan::new(
        model,
        recipes,
        SupportIntersectionPolicy::RejectUncertified,
        limits,
        &mut Cancellation::never(),
    )
}
/// Evaluate the complete eight-support scene transactionally.
pub(crate) fn evaluate(
    limits: SupportLimits,
) -> Result<SupportedModel, karasu_core::preparation::supports::SupportError> {
    generate_supports(
        &source(),
        &plan(limits)?,
        limits,
        &mut Cancellation::never(),
    )
}
/// Render one independently addressable 200-square binary layer.
pub(crate) fn pixels(model: &SupportedModel, z: f64) -> Result<Vec<u8>, Box<dyn Error>> {
    let schedule = build_layer_schedule(
        UniformScheduleSpec::try_new(z, 1., ScheduleEnd::Count(1))?,
        ScheduleLimits {
            max_layers: 1,
            max_payload_bytes: 1024,
        },
    )?;
    let config = LayerConfig {
        grid: BinaryGridSpec::try_new([0., 0.], -16)?,
        fill_rule: RegionFillRule::EvenOddStrict,
        stages: LayerStageLimits::default(),
        max_output_payload_bytes: 64 * 1024 * 1024,
    };
    let mut engine = model.layer_engine(
        &schedule,
        config,
        ConstructLimits::default(),
        Cancellation::never(),
        None,
    );
    let layer = engine
        .next()
        .ok_or_else(|| io::Error::other("missing layer"))??;
    let image = rasterize_layer(
        &layer,
        LcdProfile::try_new([200, 200], [0., 0.], [20., 20.], Orientation::default())?,
        RasterConfig {
            samples_per_axis: 1,
            grayscale_levels: 2,
            minimum_gray: 0,
            outside: OutsidePolicy::Reject,
            compensation: Compensation::None,
            kept: karasu_core::raster::KeptSampling::Antialiased,
        },
        RasterLimits::default(),
    )?;
    Ok(image.to_pixels()?)
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let dest = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("usage: preparation_stress NEW_OUTPUT_DIRECTORY"))?,
    );
    if args.next().is_some() || dest.exists() {
        return Err(io::Error::other("one absent output directory is required").into());
    }
    let started = Instant::now();
    let model = evaluate(SupportLimits::default())?;
    let elapsed = started.elapsed().as_secs_f64() * 1000.;
    let mut counts = Vec::new();
    let mut images = Vec::new();
    for (z, name) in [(0.5, "z0_5.pgm"), (11., "z11.pgm")] {
        let p = pixels(&model, z)?;
        counts.push(
            serde_json::json!({"z_mm":z,"material_pixels":p.iter().filter(|&&v|v!=0).count()}),
        );
        images.push((name, p));
    }
    std::fs::create_dir(&dest)?;
    for (name, p) in images {
        let mut f = std::fs::File::create(dest.join(name))?;
        f.write_all(b"P5\n200 200\n255\n")?;
        f.write_all(&p)?;
    }
    let report = serde_json::json!({"fixture":"eight-separated-supports-v1","elapsed_geometry_ms":elapsed,"support_count":model.certificates().len(),"component_count":model.components().len(),"plan_record":model.plan_record(),"planes":counts});
    std::fs::write(
        dest.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("{}", dest.display());
    Ok(())
}
