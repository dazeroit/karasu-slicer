//! Drain holes placed freely (TD-089, M6.4c c2): resolution on the outer or cavity surface,
//! findings along the whole hole, and cuts subtracted per layer as a union.

use karasu_core::{
    cancellation::Cancellation,
    geometry::{ObjectTransform, Point3},
    mesh::TriangleMesh,
    preparation::{
        sampled::{
            axis_tilt, hollow_mesh_sampled, map_tilt, tilted_axis, HoleCuts, HoleProbe,
            HoleSurface, ProbeLimits, SampledHollowLimits, SampledHollowedModel, MAX_HOLES,
        },
        HoleSpec, HoleTilt, HollowSpec, Operation, OperationKind, PlanLimits, PreparationPlan,
        SamplingSpec, SourceBinding, SurfaceHoleSpec,
    },
    raster::LayerCut,
    slicing::schedule::LayerPlane,
};

/// A closed, outward box.
fn cube(min: [f64; 3], max: [f64; 3]) -> TriangleMesh {
    let points = (0..8)
        .map(|i| {
            Point3::new(
                if i & 1 == 0 { min[0] } else { max[0] },
                if i & 2 == 0 { min[1] } else { max[1] },
                if i & 4 == 0 { min[2] } else { max[2] },
            )
        })
        .collect();
    let mut triangles = Vec::new();
    for q in [
        [0, 2, 3, 1],
        [4, 5, 7, 6],
        [0, 1, 5, 4],
        [2, 6, 7, 3],
        [0, 4, 6, 2],
        [1, 3, 7, 5],
    ] {
        triangles.push([q[0], q[1], q[2]]);
        triangles.push([q[0], q[2], q[3]]);
    }
    TriangleMesh::try_new(points, triangles).unwrap()
}

/// A 20 mm cube hollowed with a 2 mm wall: the cavity spans 2 to 18 mm on every axis.
fn hollow_cube() -> SampledHollowedModel {
    let plan = PreparationPlan::new(
        SourceBinding {
            original_sha256: [5; 32],
            placement: ObjectTransform::identity(),
        },
        SamplingSpec {
            cell_mm: 0.25,
            distance_error_mm: 1e-6,
            max_cells: u64::MAX,
        },
        vec![Operation {
            id: 1,
            kind: OperationKind::Hollow(HollowSpec {
                blockers: Vec::new(),
                thickness_mm: 2.0,
                guarantee: None,
            }),
        }],
        PlanLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap();
    hollow_mesh_sampled(
        &cube([0.0; 3], [20.0; 3]),
        &plan,
        SampledHollowLimits::default(),
        &mut Cancellation::never(),
    )
    .unwrap()
}

/// A 3 mm hole at `point` of `surface`, `depth` deep (or to the cavity), 1 mm margins.
fn request(point: [f64; 3], depth: Option<f64>) -> SurfaceHoleSpec {
    SurfaceHoleSpec {
        surface_mm: point,
        diameter_mm: 3.0,
        outside_mm: 1.0,
        inside_mm: 1.0,
        tilt: None,
        depth_mm: depth,
    }
}

fn never() -> Cancellation<'static> {
    Cancellation::never()
}

#[test]
fn holes_from_outside_report_what_they_reach() {
    let model = hollow_cube();
    let probe = HoleProbe::hollow(&model, ProbeLimits::default(), &mut never()).unwrap();
    assert_eq!(probe.regions().len(), 1);
    let hole = |depth| {
        let resolved = probe
            .resolve(
                request([0.0, 10.0, 10.0], Some(depth)),
                HoleSurface::Outer,
                &mut never(),
            )
            .unwrap();
        // Into the material along +X, from 1 mm outside the face.
        assert_eq!(resolved.spec.direction, [1.0, 0.0, 0.0]);
        assert_eq!(resolved.spec.entry_mm, [-1.0, 10.0, 10.0]);
        assert_eq!(resolved.frame.outward, [-1.0, 0.0, 0.0]);
        probe.findings(&resolved.spec, &mut never()).unwrap()
    };
    // Through the wall into the cavity: it drains it.
    let drains = hole(3.0);
    assert!(drains.opens_outside && drains.reaches_cavity);
    assert!(!drains.through && !drains.ends_in_material);
    assert_eq!(drains.regions, [0]);
    assert_eq!(probe.drained_regions(&[drains])[0].drained_by, [1]);
    // Only into the wall: it reaches no cavity and ends in material.
    let shallow = hole(1.0);
    assert!(shallow.opens_outside && !shallow.reaches_cavity && shallow.ends_in_material);
    assert_eq!(
        probe.drained_regions(&[shallow])[0].drained_by,
        Vec::<u64>::new()
    );
    // Across the cavity into the opposite wall.
    let deep = hole(19.0);
    assert!(deep.reaches_cavity && deep.ends_in_material && !deep.through);
    // Through the whole cube: it opens on both sides, which is allowed (TD-089).
    let through = hole(25.0);
    assert!(through.through && through.reaches_cavity && !through.ends_in_material);
}

#[test]
fn holes_from_the_cavity_go_out_through_the_wall() {
    let model = hollow_cube();
    let probe = HoleProbe::hollow(&model, ProbeLimits::default(), &mut never()).unwrap();
    // The cavity face nearest the point is at X = 2; the hole goes towards -X, out of the cube.
    let resolved = probe
        .resolve(
            request([3.0, 10.0, 10.0], Some(3.0)),
            HoleSurface::Cavity,
            &mut never(),
        )
        .unwrap();
    assert!(resolved.spec.direction[0] < -0.99, "{resolved:?}");
    assert!(resolved.frame.outward[0] > 0.99, "{resolved:?}");
    assert!(
        (resolved.frame.foot_mm[0] - 2.0).abs() < 0.2,
        "{resolved:?}"
    );
    let findings = probe.findings(&resolved.spec, &mut never()).unwrap();
    assert!(findings.opens_outside && findings.reaches_cavity && !findings.ends_in_material);
    // Too short to get out: it does not open.
    let short = probe
        .resolve(
            request([3.0, 10.0, 10.0], Some(1.0)),
            HoleSurface::Cavity,
            &mut never(),
        )
        .unwrap();
    let findings = probe.findings(&short.spec, &mut never()).unwrap();
    assert!(!findings.opens_outside && findings.ends_in_material);
}

#[test]
fn solid_models_take_holes_and_automatic_depth_needs_a_cavity() {
    let solid = cube([0.0; 3], [20.0; 3]);
    let probe = HoleProbe::solid(&solid, ProbeLimits::default(), &mut never()).unwrap();
    assert_eq!(probe.regions().len(), 0);
    let resolved = probe
        .resolve(
            request([10.0, 10.0, 20.0], Some(5.0)),
            HoleSurface::Outer,
            &mut never(),
        )
        .unwrap();
    assert_eq!(resolved.spec.direction, [0.0, 0.0, -1.0]);
    let findings = probe.findings(&resolved.spec, &mut never()).unwrap();
    assert!(findings.opens_outside && !findings.reaches_cavity && findings.ends_in_material);
    // No cavity to anchor on or to reach automatically.
    assert!(probe
        .resolve(
            request([10.0, 10.0, 20.0], Some(5.0)),
            HoleSurface::Cavity,
            &mut never()
        )
        .is_err());
    assert!(probe
        .resolve(
            request([10.0, 10.0, 20.0], None),
            HoleSurface::Outer,
            &mut never()
        )
        .is_err());
    // On a hollowed model, the automatic depth reaches 1 mm past the cavity wall (the command
    // line's surface holes).
    let model = hollow_cube();
    let hollow = HoleProbe::hollow(&model, ProbeLimits::default(), &mut never()).unwrap();
    let auto = hollow
        .resolve(
            request([10.0, 10.0, 20.0], None),
            HoleSurface::Outer,
            &mut never(),
        )
        .unwrap();
    assert!((auto.spec.length_mm - 4.0).abs() < 0.2, "{auto:?}");
}

fn vertical(x: f64, low: f64, high: f64) -> HoleSpec {
    HoleSpec {
        entry_mm: [x, 10.0, high],
        direction: [0.0, 0.0, -1.0],
        length_mm: high - low,
        diameter_mm: 3.0,
    }
}

fn plane(z: f64) -> LayerPlane {
    LayerPlane {
        index: 0,
        z_mm: z,
        nominal_height_mm: 0.05,
    }
}

#[test]
fn cuts_overlap_and_only_holes_crossing_a_layer_are_cut() {
    // Two overlapping holes, and a third one lower down.
    let cuts = HoleCuts::build(
        &[
            vertical(10.0, 10.0, 21.0),
            vertical(11.0, 12.0, 21.0),
            vertical(5.0, -1.0, 4.0),
        ],
        1e-3,
        MAX_HOLES,
        4096,
        &mut never(),
    )
    .unwrap();
    assert_eq!(cuts.len(), 3);
    assert_eq!(cuts.rings(plane(15.0)).len(), 2);
    assert_eq!(cuts.rings(plane(11.0)).len(), 1);
    assert_eq!(cuts.rings(plane(2.0)).len(), 1);
    assert_eq!(cuts.rings(plane(7.0)).len(), 0);
    assert!(cuts.cutters().triangles().len() > 3 * 8);
    // Paths are never refused; only the bound on the number of holes is.
    assert!(HoleCuts::build(&[vertical(10.0, 0.0, 1.0); 3], 1e-3, 2, 4096, &mut never()).is_err());
}

#[test]
fn overlapping_bodies_are_material_for_findings() {
    // Two overlapping boxes, as sculpted models often are: inside both, the even-odd parity is
    // even, but the material is there. A hole ending in the overlap ends in material and
    // does not pass through.
    let a = cube([0.0; 3], [20.0; 3]);
    let b = cube([10.0, 0.0, 0.0], [30.0, 20.0, 20.0]);
    let offset = u32::try_from(a.vertices().len()).unwrap();
    let solid = TriangleMesh::try_new(
        a.vertices().iter().chain(b.vertices()).copied().collect(),
        a.triangles()
            .iter()
            .copied()
            .chain(b.triangles().iter().map(|t| t.map(|v| v + offset)))
            .collect(),
    )
    .unwrap();
    let probe = HoleProbe::solid(&solid, ProbeLimits::default(), &mut never()).unwrap();
    let resolved = probe
        .resolve(
            request([15.0, 10.0, 20.0], Some(5.0)),
            HoleSurface::Outer,
            &mut never(),
        )
        .unwrap();
    let findings = probe.findings(&resolved.spec, &mut never()).unwrap();
    assert!(findings.opens_outside, "{findings:?}");
    assert!(
        !findings.through && findings.ends_in_material,
        "{findings:?}"
    );
}

#[test]
fn tilts_are_carried_between_frames_with_their_axis() {
    let tilt = HoleTilt {
        angle_deg: 30.0,
        azimuth_deg: 0.0,
    };
    // On a +Y face, leaning towards +Z; the inverse gives the tilt back.
    let axis = tilted_axis([0.0, 1.0, 0.0], tilt);
    let back = axis_tilt([0.0, 1.0, 0.0], axis).unwrap();
    assert!((back.angle_deg - 30.0).abs() < 1e-9 && back.azimuth_deg.abs() < 1e-9);
    assert!(axis_tilt([0.0, 1.0, 0.0], [0.0; 3]).is_none());
    // A quarter turn about X takes +Y to +Z and +Z to -Y: on the top face, whose reference
    // tangent is +X, the hole leans towards -Y, a three-quarter turn from +X seen from above.
    let turn = [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]];
    let moved = map_tilt(tilt, [0.0, 1.0, 0.0], turn, [0.0, 0.0, 1.0]).unwrap();
    assert!((moved.angle_deg - 30.0).abs() < 1e-9, "{moved:?}");
    assert!((moved.azimuth_deg - 270.0).abs() < 1e-9, "{moved:?}");
    let carried = turn.map(|row| (0..3).map(|k| row[k] * axis[k]).sum::<f64>());
    let axis = tilted_axis([0.0, 0.0, 1.0], moved);
    for k in 0..3 {
        assert!((axis[k] - carried[k]).abs() < 1e-9, "{axis:?} {carried:?}");
    }
}
