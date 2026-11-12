//! Repair performance budgets (M6.4d r9, TD-092, plan section 8): each stage alone on large
//! models, in release, with its time and the budget it is held to.
//!
//! Large models are not in the repository: list STL paths in `KARASU_BUDGET_MODELS`, separated
//! by `;`, and run
//! `cargo test --release -p karasu-core --test repair_budgets -- --ignored --nocapture`.
//! Budgets (per stage, on about one million triangles): exact stages 0.5 s, tolerant stages
//! 2 s; times scale with the triangle count. Rebuilding at 0.05 mm is held to 10 s on the model
//! in `KARASU_REBUILD_MODEL` (the plan's budget names the Yorick miniature).
use karasu_core::{
    cancellation::Cancellation,
    mesh::{
        stl::{import_stl_path, StlOptions, StlUnitPolicy},
        topology::TopologyLimits,
        TriangleMesh,
    },
    preparation::repair::{
        repair_source, FillHoles, NonManifold, RepairPreset, RepairSpec, Shells,
    },
};
use std::time::Instant;

/// The stages measured alone, each with its budget per million triangles.
#[allow(clippy::too_many_lines)] // One entry per stage.
fn stages() -> Vec<(&'static str, RepairSpec, f64)> {
    let off = RepairPreset::Off.spec();
    let fill = |refine: bool| FillHoles {
        split_rims: true,
        refine,
        ..FillHoles::default()
    };
    vec![
        (
            "R1 weld, exact",
            RepairSpec {
                weld_mm: Some(0.0),
                ..off
            },
            0.5,
        ),
        (
            "R1 weld, 1 um",
            RepairSpec {
                weld_mm: Some(0.001),
                ..off
            },
            2.0,
        ),
        (
            "R2 degenerate, exact",
            RepairSpec {
                degenerate_mm: Some(0.0),
                ..off
            },
            0.5,
        ),
        (
            "R2 degenerate, 1 um",
            RepairSpec {
                degenerate_mm: Some(0.001),
                ..off
            },
            2.0,
        ),
        (
            "R3+R6 duplicates, orientation",
            RepairSpec {
                duplicates: true,
                orientation: true,
                ..off
            },
            0.5,
        ),
        (
            "R5 fins",
            RepairSpec {
                non_manifold: Some(NonManifold::Fins),
                ..off
            },
            0.5,
        ),
        (
            "R6 visibility",
            RepairSpec {
                visibility: true,
                ..off
            },
            2.0,
        ),
        (
            "R4+R7 cracks, holes, exact",
            RepairSpec {
                fill_holes: Some(fill(false)),
                allow_unfilled: true,
                ..off
            },
            0.5,
        ),
        (
            "R4+R7 cracks 1 um, holes refined",
            RepairSpec {
                fill_holes: Some(fill(true)),
                crack_mm: Some(0.001),
                allow_unfilled: true,
                ..off
            },
            2.0,
        ),
        (
            "R8 shells (1 mm2, inner)",
            RepairSpec {
                shells: Some(Shells {
                    min_area_mm2: 1.0,
                    min_volume_mm3: 0.0,
                    inner: true,
                }),
                ..off
            },
            2.0,
        ),
        (
            "R6 outward",
            RepairSpec {
                orientation: true,
                outward: true,
                ..off
            },
            0.5,
        ),
        ("Preset Exact", RepairPreset::Exact.spec(), f64::NAN),
        ("Preset Standard", RepairPreset::Standard.spec(), f64::NAN),
        (
            "Preset Aggressive",
            RepairPreset::Aggressive.spec(),
            f64::NAN,
        ),
    ]
}

fn measure(mesh: &TriangleMesh, spec: RepairSpec) -> Result<f64, String> {
    let start = Instant::now();
    repair_source(
        mesh,
        spec,
        TopologyLimits::default(),
        &mut Cancellation::never(),
    )
    .map_err(|e| e.to_string())?;
    Ok(start.elapsed().as_secs_f64())
}

#[test]
#[ignore = "measures local large models; see the module documentation"]
fn repair_stages_meet_their_budgets() {
    let Ok(list) = std::env::var("KARASU_BUDGET_MODELS") else {
        eprintln!("KARASU_BUDGET_MODELS is not set: nothing to measure");
        return;
    };
    let mut over = Vec::new();
    for path in list.split(';').filter(|p| !p.is_empty()) {
        let mesh = import_stl_path(path, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
            .unwrap_or_else(|e| panic!("{path}: {e}"))
            .mesh;
        #[allow(clippy::cast_precision_loss)] // Triangle counts are far below 2^52.
        let millions = mesh.triangles().len() as f64 / 1e6;
        let name = std::path::Path::new(path)
            .file_name()
            .map_or_else(|| path.to_owned(), |n| n.to_string_lossy().into_owned());
        println!("\n{name}: {} triangles", mesh.triangles().len());
        println!("| Stage | Time (s) | Budget (s) |\n| --- | --- | --- |");
        for (stage, spec, per_million) in stages() {
            let budget = per_million * millions.max(1.0);
            match measure(&mesh, spec) {
                Ok(seconds) => {
                    let mark = if seconds > budget { " (over)" } else { "" };
                    println!(
                        "| {stage} | {seconds:.3}{mark} | {} |",
                        if budget.is_nan() {
                            "-".to_owned()
                        } else {
                            format!("{budget:.2}")
                        }
                    );
                    if seconds > budget {
                        over.push(format!("{name}: {stage} {seconds:.3} s > {budget:.2} s"));
                    }
                }
                Err(e) => println!("| {stage} | refused: {e} | |"),
            }
        }
    }
    assert!(over.is_empty(), "over budget: {over:#?}");
}

#[test]
#[ignore = "measures a local model; see the module documentation"]
fn rebuilding_meets_its_budget() {
    let Ok(path) = std::env::var("KARASU_REBUILD_MODEL") else {
        eprintln!("KARASU_REBUILD_MODEL is not set: nothing to measure");
        return;
    };
    let mesh = import_stl_path(&path, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .mesh;
    let spec = RepairSpec {
        rebuild_mm: Some(0.05),
        ..RepairPreset::Off.spec()
    };
    let seconds = measure(&mesh, spec).unwrap();
    println!(
        "{path}: {} triangles, rebuilt at 0.05 mm in {seconds:.3} s",
        mesh.triangles().len()
    );
    assert!(seconds <= 10.0, "{seconds:.3} s > 10 s");
}
