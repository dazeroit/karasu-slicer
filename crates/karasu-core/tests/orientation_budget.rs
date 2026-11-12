//! Orientation's measures on a local large model (TD-119): set `KARASU_ORIENT_MODEL` to an STL
//! file in millimetres. Run in release; it prints the time of each pass.
use karasu_core::{
    analysis::orientation::{
        distinct, largest_section_mm2, rank, rough, Measures, RoughRequest, TiltAxes, Weights,
    },
    cancellation::Cancellation,
    mesh::stl::{import_stl_path, StlOptions, StlUnitPolicy},
};
use std::time::Instant;

#[test]
#[ignore = "measures a local large model; see the module documentation"]
fn a_local_model_is_oriented_within_its_budget() {
    let Ok(path) = std::env::var("KARASU_ORIENT_MODEL") else {
        eprintln!("KARASU_ORIENT_MODEL is not set: nothing to measure");
        return;
    };
    let mesh = import_stl_path(&path, StlOptions::new(StlUnitPolicy::AssumeMillimeters))
        .unwrap()
        .mesh;
    let weights = Weights {
        facing: 1.0,
        islands: 1.0,
        cups: 2.0,
        height: 0.5,
        largest: 0.5,
        protected: 4.0,
    };
    for (axes, most) in [
        (TiltAxes::Any, Some(30.0)),
        (TiltAxes::Any, None),
        (TiltAxes::X, None),
    ] {
        let started = Instant::now();
        let found = rough(
            &RoughRequest {
                mesh: &mesh,
                angle_deg: 45.0,
                axes,
                max_tilt_deg: most,
                protected: &[],
                keep_away_mm: 5.0,
            },
            &mut Cancellation::never(),
        )
        .unwrap();
        let rough_s = started.elapsed().as_secs_f64();
        let measures: Vec<Measures> = found.iter().map(Measures::of).collect();
        let tilts: Vec<_> = found.iter().map(|r| r.tilt).collect();
        let best = distinct(&tilts, &rank(&measures, &weights), 8, 8.0);
        let started = Instant::now();
        let largest: Vec<f64> = best
            .iter()
            .map(|&i| {
                largest_section_mm2(&mesh, tilts[i], 0.5, &mut Cancellation::never()).unwrap()
            })
            .collect();
        let sections_s = started.elapsed().as_secs_f64();
        println!(
            "{} triangles, {axes:?} within {most:?}: {} tilts in {rough_s:.3} s; sections of the best {} in {sections_s:.3} s",
            mesh.triangles().len(),
            found.len(),
            best.len()
        );
        for (k, &i) in best.iter().enumerate() {
            let r = &found[i];
            println!(
                "  {:5.1}° facing {:8.1} mm² lowest {:5} height {:6.2} mm largest {:7.1} mm²",
                r.tilt.angle_deg(),
                r.facing_mm2,
                r.lowest_points,
                r.height_mm,
                largest[k]
            );
        }
    }
}
