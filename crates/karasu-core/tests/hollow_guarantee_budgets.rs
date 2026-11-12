//! Wall-guarantee measurements (M6.4e, TD-093): each model hollowed with the guarantee's cell and
//! offset at several walls, in release, with its passes, certified wall, cavity size and time.
//!
//! Large models are not in the repository: list STL paths in `KARASU_GUARANTEE_MODELS`,
//! separated by `;`, optionally the walls in `KARASU_GUARANTEE_WALLS` (default `2.0;1.3;1.0;0.5`),
//! and run
//! `cargo test --release -p karasu-core --test hollow_guarantee_budgets -- --ignored --nocapture`.
use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3, Rotation3, Vector3},
    mesh::stl::{import_stl_path, StlOptions, StlUnitPolicy},
    preparation::{
        sampled::{
            guaranteed_cell, hollow_mesh_sampled, surface_area, SampledHollowLimits,
            DEFAULT_CAVITY_TRIANGLE_BUDGET, DEFAULT_OVER_THICKNESS_MM,
            DEFAULT_SHORTFALL_COEFFICIENT,
        },
        HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan, SamplingSpec,
        SourceBinding, WallGuarantee,
    },
};
use std::time::Instant;

const ERROR_MM: f64 = 1e-6;

fn plan(wall: f64, cell: f64) -> PreparationPlan {
    PreparationPlan::new(
        SourceBinding {
            original_sha256: [0; 32],
            placement: ObjectTransform::new(
                Vector3::new(0.0, 0.0, 0.0),
                Rotation3::identity(),
                Vector3::new(1.0, 1.0, 1.0),
                Point3::new(0.0, 0.0, 0.0),
            )
            .unwrap(),
        },
        SamplingSpec {
            cell_mm: cell,
            distance_error_mm: ERROR_MM,
            max_cells: u64::MAX,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm: wall,
                guarantee: Some(WallGuarantee {
                    shortfall_coefficient: DEFAULT_SHORTFALL_COEFFICIENT,
                }),
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

#[test]
#[ignore = "measures local large models; see the module documentation"]
fn guaranteed_walls_are_reached() {
    let Ok(list) = std::env::var("KARASU_GUARANTEE_MODELS") else {
        eprintln!("KARASU_GUARANTEE_MODELS is not set: nothing to measure");
        return;
    };
    let walls: Vec<f64> = std::env::var("KARASU_GUARANTEE_WALLS")
        .unwrap_or_else(|_| "2.0;1.3;1.0;0.5".to_owned())
        .split(';')
        .map(|w| w.parse().unwrap())
        .collect();
    let mut failures = Vec::new();
    println!(
        "| Model | Wall | Cell | Passes | Certified wall | Thickest at vertices | Cavity triangles | Time (s) |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    for path in list.split(';').filter(|p| !p.is_empty()) {
        let mesh = import_stl_path(path, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .mesh;
        let name = std::path::Path::new(path)
            .file_name()
            .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        let area = surface_area(&mesh);
        for &wall in &walls {
            let cell = guaranteed_cell(
                area,
                wall,
                ERROR_MM,
                DEFAULT_OVER_THICKNESS_MM,
                DEFAULT_CAVITY_TRIANGLE_BUDGET,
            );
            let start = Instant::now();
            let result = hollow_mesh_sampled(
                &mesh,
                &plan(wall, cell),
                SampledHollowLimits::default(),
                &mut Cancellation::never(),
            );
            let seconds = start.elapsed().as_secs_f64();
            match result {
                Ok(model) => {
                    let c = model.certificate();
                    println!(
                        "| {name} | {wall} | {cell:.3} | {} | {:.4} | {:.4} | {} | {seconds:.2} |",
                        c.passes,
                        c.surface_distance_mm[0],
                        c.vertex_distance_mm[1],
                        model.cavity().triangles().len()
                    );
                    if c.surface_distance_mm[0] + 2.0 * c.query_error_mm < wall {
                        failures.push(format!("{name} at {wall}: {c:?}"));
                    }
                }
                Err(e) => {
                    println!("| {name} | {wall} | {cell:.3} | refused: {e} | | | | {seconds:.2} |");
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
