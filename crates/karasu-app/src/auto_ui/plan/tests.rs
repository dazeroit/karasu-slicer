use super::*;
use crate::{preparation_ui::PreparationUi, supports_ui::SupportsUi, workspace::Workspace};
use std::time::{Duration, Instant};

/// A slab 20 × 20 × 3 mm whose bottom is at 15 mm, on a pillar under its first 4 mm along X: an
/// overhang of 16 × 20 mm with nothing under it. Placed on a preset printer, prepared, with its
/// surface for routes; and the slab's bottom corner on the plate.
fn slab() -> (Workspace, Arc<RouteSurface>, [f64; 3], std::path::PathBuf) {
    // Tests run in parallel and remove their directory: each call gets its own.
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("karasu-auto-plan-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("slab.stl");
    let pillar = crate::workspace::tests::box_stl([0.0; 3], [4.0, 20.0, 16.0]);
    let inner = pillar
        .trim_start_matches("solid box\n")
        .trim_end_matches("endsolid box\n");
    let slab = crate::workspace::tests::box_stl([0.0, 0.0, 15.0], [20.0, 20.0, 18.0]);
    std::fs::write(
        &path,
        slab.replace("endsolid box\n", &format!("{inner}endsolid box\n")),
    )
    .unwrap();
    let (workspace, preparation) =
        crate::hollowing_ui::tests::placed(path, None, Duration::from_secs(60));
    let surface = surface_of(&workspace, &preparation);
    let min = workspace.placement_of(0).unwrap().min.coordinates();
    (
        workspace,
        surface,
        [min[0], min[1], min[2] + 15.0],
        directory,
    )
}

/// The model's surface for routes, gridded within a minute.
fn surface_of(workspace: &Workspace, preparation: &PreparationUi) -> Arc<RouteSurface> {
    let mut supports = SupportsUi::new();
    supports.route_wanted = true;
    let started = Instant::now();
    loop {
        supports.poll(workspace, preparation, Some(0));
        if let Some(surface) = supports.route_surface(workspace, preparation, 0) {
            return surface;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "no grid");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The slab's bottom beyond the pillar, as the analysis reports an overhang there.
fn overhang(corner: [f64; 3]) -> (RegionKind, Island) {
    let [x, y, z] = corner;
    (
        RegionKind::Overhang,
        Island {
            point: [x + 12.0, y + 10.0, z],
            layers: [300, 301],
            heights: [z - 0.05, z + 0.05],
            area_mm2: 320.0,
            footprint: [[x + 4.0, y], [x + 20.0, y + 20.0]],
            inside: false,
        },
    )
}

/// What the slab's plan is made from: `values`, no braces.
fn input(
    workspace: &Workspace,
    surface: Arc<RouteSurface>,
    regions: Vec<(RegionKind, Island)>,
    values: AutoSupports,
    kept_tips: Vec<[f64; 3]>,
) -> Input {
    let printer = workspace.printer().unwrap();
    Input {
        regions,
        islands: Vec::new(),
        document: None,
        kept_tips,
        surface,
        next: values.next(&[]),
        cavity_next: values.cavity().map(|cavity| cavity.next(&[])),
        values,
        angle_deg: 45.0,
        volume: BuildVolume {
            origin_xy_mm: printer.origin_xy_mm,
            size_mm: printer.build_size_mm,
        },
        to_model: workspace.model_to_world(0).unwrap().inverse().unwrap(),
        model: None,
    }
}

/// The values without trees or braces: a support of its own under every tip, at the spacing as
/// set (nothing closer under wide undersides).
fn plain() -> AutoSupports {
    AutoSupports {
        branches: false,
        braces: false,
        wide: false,
        ..AutoSupports::default()
    }
}

/// The plain values' contact's radius at the surface (Medium's, TD-136).
fn radius() -> f64 {
    plain().next(&[]).parameters().surface_width_mm() / 2.0
}

/// The tips `values` plan under the slab's overhang.
fn tips_under_slab(values: AutoSupports) -> Vec<[f64; 3]> {
    let (workspace, surface, corner, directory) = slab();
    let made = plan(
        &input(
            &workspace,
            surface,
            vec![overhang(corner)],
            values,
            Vec::new(),
        ),
        &|_| {},
        &AtomicBool::new(false),
    );
    let [covered, all] = made.overhangs;
    assert_eq!(covered, all, "{values:?}");
    let _ = std::fs::remove_dir_all(directory);
    made.plan
        .supports
        .iter()
        .map(|p| p.routed.support.tip_mm)
        .collect()
}

/// The least distance between two of `tips`.
fn closest(tips: &[[f64; 3]]) -> f64 {
    let mut least = f64::INFINITY;
    for (k, p) in tips.iter().enumerate() {
        for q in &tips[k + 1..] {
            least = least.min((p[0] - q[0]).hypot(p[1] - q[1]).hypot(p[2] - q[2]));
        }
    }
    least
}

#[test]
fn density_and_wide_undersides_bring_the_tips_closer() {
    // TD-127: density scales the tips per area, the spacing by sqrt(100 / density).
    let values = plain();
    assert!((values.density_scale() - 1.0).abs() < 1e-12);
    let double = AutoSupports {
        density_pct: 200.0,
        ..values
    };
    assert!((double.density_scale() - 0.5f64.sqrt()).abs() < 1e-12);
    // Under a wide underside: sqrt(from / area), no closer than the share set; off, or no
    // wider than the area set, the spacing as set.
    let wide = AutoSupports {
        wide: true,
        ..values
    };
    assert!((wide.wide_factor(80.0) - 0.6).abs() < 1e-12);
    assert!((wide.wide_factor(25.0) - (20.0f64 / 25.0).sqrt()).abs() < 1e-12);
    assert!((wide.wide_factor(20.0) - 1.0).abs() < 1e-12);
    assert!((values.wide_factor(1000.0) - 1.0).abs() < 1e-12);
    // Out of range, the built-in values.
    let wrong = AutoSupports {
        density_pct: 300.0,
        ..values
    };
    assert_eq!(wrong.valid(), DEFAULT_AUTO_SUPPORTS);
    // Under the slab's 320 mm² overhang: twice the density, about twice the tips, no two closer
    // than the scaled spacing; the wide underside's 60% brings them closer still.
    let normal = tips_under_slab(values);
    let dense = tips_under_slab(double);
    let closer = tips_under_slab(wide);
    let ratio = |more: &[[f64; 3]]| {
        #[allow(clippy::cast_precision_loss)] // Tips.
        let r = more.len() as f64 / normal.len() as f64;
        r
    };
    assert!(
        (1.6..=2.6).contains(&ratio(&dense)),
        "{} {}",
        dense.len(),
        normal.len()
    );
    assert!(closest(&dense) >= values.spacing_mm * 0.5f64.sqrt() - 1e-9);
    assert!(
        (2.0..=3.6).contains(&ratio(&closer)),
        "{} {}",
        closer.len(),
        normal.len()
    );
    assert!(closest(&closer) >= values.spacing_mm * 0.6 - 1e-9);
}

#[test]
fn an_overhang_is_covered_by_tips_no_closer_than_the_spacing() {
    let (workspace, surface, corner, directory) = slab();
    let values = plain();
    let made = plan(
        &input(
            &workspace,
            surface,
            vec![overhang(corner)],
            values,
            Vec::new(),
        ),
        &|_| {},
        &AtomicBool::new(false),
    );
    let tips: Vec<[f64; 3]> = made
        .plan
        .supports
        .iter()
        .map(|p| p.routed.support.tip_mm)
        .collect();
    // Every point of the level underside within 2.5 mm of a tip, and nothing left.
    let [covered, all] = made.overhangs;
    assert!(all > 100, "{all} points");
    assert_eq!(covered, all);
    assert!(made.left.is_empty(), "{:?}", made.left);
    // Under the slab beyond the pillar, each tip at least the spacing from the others.
    assert!((20..=120).contains(&tips.len()), "{} tips", tips.len());
    for (k, p) in tips.iter().enumerate() {
        assert!((p[2] - corner[2]).abs() < 0.01, "{p:?}");
        assert!(p[0] > corner[0] + 4.0 - 1e-6, "{p:?}");
        for q in &tips[k + 1..] {
            let d = (p[0] - q[0]).hypot(p[1] - q[1]).hypot(p[2] - q[2]);
            assert!(d >= values.spacing_mm - 1e-9, "{p:?} {q:?}");
        }
    }
    assert!(made
        .plan
        .supports
        .iter()
        .all(|p| p.routed.support.origin == SupportOrigin::Automatic));
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn islands_only_leaves_the_overhangs_and_kept_tips_keep_their_room() {
    let (workspace, surface, corner, directory) = slab();
    // Islands only: an overhang gets nothing.
    let only = AutoSupports {
        cover: Cover::IslandsOnly,
        ..plain()
    };
    let made = plan(
        &input(
            &workspace,
            Arc::clone(&surface),
            vec![overhang(corner)],
            only,
            Vec::new(),
        ),
        &|_| {},
        &AtomicBool::new(false),
    );
    assert!(made.plan.supports.is_empty());
    assert_eq!(made.overhangs, [0, 0]);
    // A support placed by hand under the middle: no new tip within the spacing of it.
    let middle = [corner[0] + 12.0, corner[1] + 10.0, corner[2]];
    let made = plan(
        &input(
            &workspace,
            surface,
            vec![overhang(corner)],
            plain(),
            vec![middle],
        ),
        &|_| {},
        &AtomicBool::new(false),
    );
    assert!(!made.plan.supports.is_empty());
    for p in made.plan.supports.iter().map(|p| p.routed.support.tip_mm) {
        let d = (p[0] - middle[0]).hypot(p[1] - middle[1]);
        assert!(d >= plain().spacing_mm - 1e-9, "{p:?}");
    }
    assert_eq!(made.overhangs[0], made.overhangs[1]);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn a_plan_of_the_cavity_alone_leaves_the_outside_as_it_is() {
    let (workspace, surface, corner, directory) = slab();
    // The slab's underside is outside: the cavity alone covers nothing of it, nor an island
    // there; the outside alone covers it as everywhere does.
    let island = overhang(corner).1;
    let run = |place: Place| {
        let mut made = input(
            &workspace,
            Arc::clone(&surface),
            vec![overhang(corner)],
            AutoSupports { place, ..plain() },
            Vec::new(),
        );
        made.islands = vec![island];
        plan(&made, &|_| {}, &AtomicBool::new(false))
    };
    let cavity = run(Place::Cavity);
    assert!(cavity.plan.supports.is_empty());
    assert_eq!((cavity.overhangs, cavity.islands), ([0, 0], [0, 0]));
    let [outside, everywhere] = [Place::Outside, Place::Everywhere].map(run);
    assert!(!outside.plan.supports.is_empty());
    assert_eq!(outside.plan.supports.len(), everywhere.plan.supports.len());
    assert_eq!(outside.overhangs, everywhere.overhangs);
    assert_eq!(outside.islands, everywhere.islands);
    // Place::holds: the cavity and the outside apart, everywhere both.
    for (place, held) in [
        (Place::Everywhere, [true, true]),
        (Place::Outside, [true, false]),
        (Place::Cavity, [false, true]),
    ] {
        assert_eq!([place.holds(false), place.holds(true)], held, "{place:?}");
    }
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn settings_that_knew_only_inside_read_as_a_place() {
    let read = |json: &str| serde_json::from_str::<AutoSupports>(json).unwrap().place;
    assert_eq!(read(r#"{"inside": true}"#), Place::Everywhere);
    assert_eq!(read(r#"{"inside": false}"#), Place::Outside);
    assert_eq!(read(r#"{"place": "cavity"}"#), Place::Cavity);
    assert_eq!(read("{}"), Place::Everywhere);
    let saved = serde_json::to_string(&AutoSupports {
        place: Place::Cavity,
        ..AutoSupports::default()
    })
    .unwrap();
    assert!(saved.contains(r#""place":"cavity""#), "{saved}");
    assert_eq!(read(&saved), Place::Cavity);
}

#[test]
fn trees_grow_and_the_plan_is_the_same_every_time() {
    let (workspace, surface, corner, directory) = slab();
    let values = AutoSupports {
        braces: false,
        ..AutoSupports::default()
    };
    let run = || {
        plan(
            &input(
                &workspace,
                Arc::clone(&surface),
                vec![overhang(corner)],
                values,
                Vec::new(),
            ),
            &|_| {},
            &AtomicBool::new(false),
        )
    };
    let (a, b) = (run(), run());
    assert!(a.plan.branches > 0);
    let tips = |o: &AutoOutcome| -> Vec<_> {
        o.plan
            .supports
            .iter()
            .map(|p| (p.routed.support.clone(), p.joins))
            .collect()
    };
    assert_eq!(tips(&a), tips(&b));
    assert_eq!(a.overhangs[0], a.overhangs[1]);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn no_tip_goes_where_a_drain_hole_cuts_the_overhang_away() {
    // TD-131: a 4 mm hole up through the slab's overhang. The mesh still has the surface there,
    // but the hole removes it: nothing to hold, so no candidate; around it, the same as without.
    let (workspace, surface, corner, directory) = slab();
    let mesh = surface.mesh().unwrap();
    let [x, y, z] = corner;
    let centre = [x + 12.0, y + 10.0];
    let cut = DrainCylinder {
        start_mm: [centre[0], centre[1], z - 1.0],
        end_mm: [centre[0], centre[1], z + 2.0],
        radius_mm: 2.0,
    };
    let regions = [overhang(corner)];
    let points = |cuts: &[DrainCylinder], rank: Option<Rank>| -> Vec<[f64; 3]> {
        candidates(&surface, mesh, &regions, cuts, (&plain(), radius()), 45.0)
            .candidates
            .iter()
            .filter(|c| rank.is_none_or(|r| c.rank == r))
            .map(|c| c.tip.0)
            .collect()
    };
    let from_centre = |p: &[f64; 3]| (p[0] - centre[0]).hypot(p[1] - centre[1]);
    let inside = |p: &[f64; 3]| from_centre(p) < 2.0;
    let all = points(&[], None);
    let cut_away = points(&[cut], None);
    assert!(all.iter().any(inside), "{all:?}");
    assert!(!cut_away.iter().any(inside), "{cut_away:?}");
    // Inside the overhang, the same points as without the hole.
    let kept: Vec<_> = points(&[], Some(Rank::Inside))
        .into_iter()
        .filter(|p| !inside(p))
        .collect();
    assert_eq!(points(&[cut], Some(Rank::Inside)), kept);
    // The hole's rim is an edge (TD-136): its edge points stand the inset beyond it along X or
    // Y, so at least half of it from the rim, and the inset exactly where the rim crosses an axis.
    let step = plain().spacing_mm / 2.0;
    let rim: Vec<_> = points(&[cut], Some(Rank::Edge))
        .into_iter()
        .filter(|p| from_centre(p) < 2.0 + step + EDGE_INSET_MM)
        .collect();
    assert!(rim.len() >= 4, "{rim:?}");
    assert!(
        rim.iter()
            .all(|p| from_centre(p) >= 2.0 + EDGE_INSET_MM / 2.0),
        "{rim:?}"
    );
    let on_axis =
        |p: &&[f64; 3]| (p[1] - centre[1]).abs() < 1e-9 || (p[0] - centre[0]).abs() < 1e-9;
    assert!(rim.iter().filter(on_axis).count() >= 2, "{rim:?}");
    for p in rim.iter().filter(on_axis) {
        assert!((from_centre(p) - 2.0 - EDGE_INSET_MM).abs() < 0.05, "{p:?}");
    }
    let _ = std::fs::remove_dir_all(directory);
    drop(workspace);
}

/// A test plate's tile (15 × 15 × 2 mm) lifted 8 mm, turned `tilt_deg` about X, prepared with
/// its surface; and its underside as the analysis reports a region there.
fn tile(
    tilt_deg: f64,
) -> (
    Workspace,
    Arc<RouteSurface>,
    (RegionKind, Island),
    std::path::PathBuf,
) {
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let call = CALLS.fetch_add(1, Ordering::Relaxed);
    let directory =
        std::env::temp_dir().join(format!("karasu-auto-tile-{}-{call}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("tile.stl");
    std::fs::write(
        &path,
        crate::workspace::tests::box_stl([0.0; 3], [15.0, 15.0, 2.0]),
    )
    .unwrap();
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(path, None, Duration::from_secs(60));
    workspace.edit(|plate| {
        let pose = &mut plate.objects[0].pose;
        pose.rotate_deg = [tilt_deg, 0.0, 0.0];
        pose.center = true;
        pose.drop = true;
        pose.lift_mm = Some(8.0);
    });
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(60),
    );
    let surface = surface_of(&workspace, &preparation);
    let placed = workspace.placement_of(0).unwrap();
    let (min, max) = (placed.min.coordinates(), placed.max.coordinates());
    let rise = 15.0 * tilt_deg.to_radians().sin();
    let region = Island {
        point: [
            f64::midpoint(min[0], max[0]),
            f64::midpoint(min[1], max[1]),
            min[2],
        ],
        layers: [160, 161],
        heights: [min[2] - 0.05, min[2] + rise + 0.05],
        area_mm2: 225.0,
        footprint: [[min[0], min[1]], [max[0], max[1]]],
        inside: false,
    };
    (workspace, surface, (RegionKind::Island, region), directory)
}

#[test]
fn an_underside_s_edges_and_corners_take_tips_the_inset_in() {
    // TD-136: the test tiles held flat on tips 0.4 mm in from their edges and corners.
    let (workspace, surface, region, directory) = tile(0.0);
    let mesh = surface.mesh().unwrap();
    let [[x0, y0], [x1, y1]] = region.1.footprint;
    let probed = candidates(&surface, mesh, &[region], &[], (&plain(), radius()), 45.0);
    assert!(
        probed.sides.is_empty(),
        "a level underside has no leading edge"
    );
    let ranked = |rank: Rank| -> Vec<[f64; 3]> {
        probed
            .candidates
            .iter()
            .filter(|c| c.rank == rank)
            .map(|c| c.tip.0)
            .collect()
    };
    let inset = EDGE_INSET_MM;
    let near = |a: f64, b: f64| (a - b).abs() < 0.03;
    // A corner tip at each corner, the inset in from both edges.
    let corners = ranked(Rank::Corner);
    for [cx, cy] in [
        [x0 + inset, y0 + inset],
        [x1 - inset, y0 + inset],
        [x0 + inset, y1 - inset],
        [x1 - inset, y1 - inset],
    ] {
        assert!(
            corners.iter().any(|p| near(p[0], cx) && near(p[1], cy)),
            "{corners:?}"
        );
    }
    // Every edge tip the inset in from one edge, and every edge has them.
    let edges = ranked(Rank::Edge);
    let from_edge = |p: &[f64; 3]| [p[0] - x0, x1 - p[0], p[1] - y0, y1 - p[1]];
    assert!(edges
        .iter()
        .all(|p| from_edge(p).iter().any(|&d| near(d, inset))));
    for side in 0..4 {
        assert!(
            edges
                .iter()
                .filter(|p| near(from_edge(p)[side], inset))
                .count()
                >= 4,
            "{side}"
        );
    }
    // The plan takes the corners and an edge tip along each edge, none closer than the
    // spacing, and covers every point.
    let made = plan(
        &input(
            &workspace,
            Arc::clone(&surface),
            vec![region],
            plain(),
            Vec::new(),
        ),
        &|_| {},
        &AtomicBool::new(false),
    );
    let tips: Vec<[f64; 3]> = made
        .plan
        .supports
        .iter()
        .map(|p| p.routed.support.tip_mm)
        .collect();
    assert_eq!(made.overhangs[0], made.overhangs[1]);
    assert!(made.left.is_empty(), "{:?}", made.left);
    for c in &corners {
        assert!(tips.iter().any(|t| distance(*t, *c) < 1e-6), "{c:?}");
    }
    assert!(closest(&tips) >= plain().spacing_mm - 1e-9);
    let outer = tips
        .iter()
        .filter(|p| from_edge(p).iter().any(|&d| near(d, inset)))
        .count();
    assert!(outer >= 16, "{outer} of {} tips at the edges", tips.len());
    let _ = std::fs::remove_dir_all(directory);
    drop(workspace);
}

#[test]
fn where_the_surface_goes_on_there_is_no_edge() {
    // The slab's overhang ends at the pillar, where its bottom goes on over it: no edge tip
    // there; its three free sides have them.
    let (workspace, surface, corner, directory) = slab();
    let mesh = surface.mesh().unwrap();
    let probed = candidates(
        &surface,
        mesh,
        &[overhang(corner)],
        &[],
        (&plain(), radius()),
        45.0,
    );
    let edges: Vec<[f64; 3]> = probed
        .candidates
        .iter()
        .filter(|c| c.rank != Rank::Inside)
        .map(|c| c.tip.0)
        .collect();
    assert_ne!(edges.len(), 0);
    let [x, y, _] = corner;
    let from_edge = |p: &[f64; 3]| [x + 20.0 - p[0], p[1] - y, y + 20.0 - p[1]];
    // Each the inset in from a free side, none the inset in from the pillar's.
    assert!(
        edges.iter().all(|p| from_edge(p)
            .iter()
            .any(|d| (d - EDGE_INSET_MM).abs() < 0.03)
            && from_edge(p).iter().all(|d| *d > EDGE_INSET_MM - 0.03)),
        "{edges:?}"
    );
    assert!(
        edges
            .iter()
            .all(|p| p[0] - (x + 4.0) > EDGE_INSET_MM + 0.05),
        "{edges:?}"
    );
    let _ = std::fs::remove_dir_all(directory);
    drop(workspace);
}

#[test]
fn a_leading_edge_is_held_from_its_side_too() {
    // TD-136 (owner, 2026-11-06): the tilted tiles bent at their lowest edge, which prints first;
    // its tips go as close to it as supports placed by hand, and the face beside it takes side
    // tips between them, which the plan holds.
    let (workspace, surface, region, directory) = tile(30.0);
    let mesh = surface.mesh().unwrap();
    let probed = candidates(&surface, mesh, &[region], &[], (&plain(), radius()), 45.0);
    let low = region.1.heights[0] + 0.05;
    let (sin, cos) = 30f64.to_radians().sin_cos();
    // Under the edge, away from the tile's sides: the contact's radius up the slope from it.
    let [[x0, _], [x1, _]] = region.1.footprint;
    let under: Vec<[f64; 3]> = probed
        .candidates
        .iter()
        .filter(|c| {
            let p = c.tip.0;
            c.rank == Rank::Edge && p[2] < low + 0.5 && p[0] > x0 + 1.0 && p[0] < x1 - 1.0
        })
        .map(|c| c.tip.0)
        .collect();
    assert!(under.len() >= 5, "{under:?}");
    for p in &under {
        assert!(((p[2] - low) / sin - radius()).abs() < 0.03, "{p:?}");
    }
    assert!(probed.sides.len() >= 10, "{}", probed.sides.len());
    for c in &probed.sides {
        let (p, n, _) = c.tip;
        // On the face beside the lowest edge, steeper than the critical angle: the contact's
        // radius up it, within a probe step.
        let tilt = tilt(n).to_degrees();
        assert!((45.0..=75.0).contains(&tilt), "{tilt} at {p:?}");
        let up = (p[2] - low) / cos;
        assert!(
            up >= radius() - 1e-6 && up < radius() + 0.1,
            "{up} at {p:?}"
        );
    }
    let made = plan(
        &input(
            &workspace,
            Arc::clone(&surface),
            vec![region],
            plain(),
            Vec::new(),
        ),
        &|_| {},
        &AtomicBool::new(false),
    );
    assert!(made.left.is_empty(), "{:?}", made.left);
    let tips: Vec<[f64; 3]> = made
        .plan
        .supports
        .iter()
        .map(|p| p.routed.support.tip_mm)
        .collect();
    // Along the 15 mm edge, the tips under it and the side tips alternate, as supports placed by
    // hand do: each side tip between two tips under the edge, one in each gap at most, and half
    // the spacing or more from every other tip.
    let sides: Vec<[f64; 3]> = tips
        .iter()
        .copied()
        .filter(|t| probed.sides.iter().any(|c| distance(*t, c.tip.0) < 1e-6))
        .collect();
    let mut held_under: Vec<f64> = tips
        .iter()
        .filter(|t| {
            t[2] < low + 0.5
                && probed
                    .candidates
                    .iter()
                    .any(|c| c.rank != Rank::Inside && distance(**t, c.tip.0) < 1e-6)
        })
        .map(|t| t[0])
        .collect();
    held_under.sort_by(f64::total_cmp);
    assert!(held_under.len() >= 4, "{held_under:?}");
    assert!(
        sides.len() >= 3,
        "{} side tips of {}",
        sides.len(),
        tips.len()
    );
    let gap = |x: f64| held_under.iter().position(|&u| u > x);
    let mut gaps: Vec<_> = sides.iter().map(|p| gap(p[0])).collect();
    assert!(
        gaps.iter().all(|g| g.is_some_and(|g| g > 0)),
        "{sides:?} {held_under:?}"
    );
    gaps.sort_unstable();
    gaps.dedup();
    assert_eq!(gaps.len(), sides.len(), "{sides:?} {held_under:?}");
    for p in &sides {
        for q in tips.iter().filter(|q| distance(**q, *p) > 1e-6) {
            assert!(distance(*p, *q) > plain().spacing_mm / 2.0, "{p:?} {q:?}");
        }
    }
    // A level tile has none.
    let (flat, surface, region, other) = tile(0.0);
    let mesh = surface.mesh().unwrap();
    let level = candidates(&surface, mesh, &[region], &[], (&plain(), radius()), 45.0);
    assert_eq!(level.sides.len(), 0);
    let _ = std::fs::remove_dir_all(directory);
    let _ = std::fs::remove_dir_all(other);
    drop((workspace, flat));
}

#[test]
#[allow(clippy::too_many_lines)] // A hollowed cube made, then four plans of it compared.
fn a_cavity_with_its_own_values_is_planned_with_them_and_the_outside_with_its_own() {
    // TD-137: a 20 mm cube lifted 15 mm, hollowed with 2 mm walls; its bottom outside and its
    // cavity's ceiling inside.
    let directory = std::env::temp_dir().join(format!("karasu-auto-cavity-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("cube.stl");
    std::fs::write(
        &path,
        crate::workspace::tests::box_stl([0.0; 3], [20.0, 20.0, 20.0]),
    )
    .unwrap();
    let (mut workspace, mut preparation) =
        crate::hollowing_ui::tests::placed(path, None, Duration::from_secs(60));
    workspace.edit(|plate| {
        let object = &mut plate.objects[0];
        object.pose.center = true;
        object.pose.drop = true;
        object.pose.lift_mm = Some(15.0);
        object.set_hollowing(Some(karasu_project::project::Hollowing {
            wall_mm: 2.0,
            cell_mm: Some(0.25),
            simplify: None,
        }));
    });
    crate::hollowing_ui::tests::prepared_within(
        &workspace,
        &mut preparation,
        Duration::from_secs(120),
    );
    let surface = surface_of(&workspace, &preparation);
    let [x0, y0, z0] = workspace.placement_of(0).unwrap().min.coordinates();
    let region = |heights: [f64; 2], inside: bool| {
        (
            RegionKind::Overhang,
            Island {
                point: [x0 + 10.0, y0 + 10.0, heights[0]],
                layers: [300, 301],
                heights,
                area_mm2: 256.0,
                footprint: [[x0 + 1.0, y0 + 1.0], [x0 + 19.0, y0 + 19.0]],
                inside,
            },
        )
    };
    let regions = vec![
        region([z0 - 0.05, z0 + 0.05], false),
        region([z0 + 17.0, z0 + 18.6], true),
    ];
    let run = |values: AutoSupports| {
        plan(
            &input(
                &workspace,
                Arc::clone(&surface),
                regions.clone(),
                values,
                Vec::new(),
            ),
            &|_| {},
            &AtomicBool::new(false),
        )
    };
    let in_cavity = |o: &AutoOutcome| -> Vec<f64> {
        o.plan
            .supports
            .iter()
            .filter(|p| p.routed.support.tip_mm[2] > z0 + 10.0)
            .map(|p| p.routed.support.parameters.contact_diameter_mm)
            .collect()
    };
    let outside_of = |o: &AutoOutcome| -> Vec<f64> {
        o.plan
            .supports
            .iter()
            .filter(|p| p.routed.support.tip_mm[2] < z0 + 1.0)
            .map(|p| p.routed.support.parameters.contact_diameter_mm)
            .collect()
    };
    // Off: one plan, the cavity as the outside.
    let same = run(plain());
    let light = karasu_core::supports::Preset::Light
        .parameters()
        .contact_diameter_mm;
    let medium = karasu_core::supports::Preset::Medium
        .parameters()
        .contact_diameter_mm;
    assert!(
        !in_cavity(&same).is_empty()
            && in_cavity(&same).iter().all(|&d| (d - medium).abs() < 1e-12)
    );
    // On, Light at half the density in the cavity: the outside as before, the cavity Light and
    // sparser.
    let own = AutoSupports {
        cavity_own: true,
        cavity_weight: Weight::Light,
        cavity_density_pct: 50.0,
        ..plain()
    };
    assert!(own.cavity().is_some_and(|c| c.place == Place::Cavity));
    let split = run(own);
    assert_eq!(outside_of(&split), outside_of(&same));
    let (before, after) = (in_cavity(&same).len(), in_cavity(&split).len());
    assert!(after > 0 && after < before, "{after} of {before}");
    assert!(in_cavity(&split).iter().all(|&d| (d - light).abs() < 1e-12));
    assert_eq!(split.overhangs[0], split.overhangs[1], "{:?}", split.left);
    // The cavity alone takes its own values too, the outside alone the outside's.
    let cavity = run(AutoSupports {
        place: Place::Cavity,
        ..own
    });
    assert_eq!(in_cavity(&cavity), in_cavity(&split));
    assert_eq!(outside_of(&cavity).len(), 0);
    let outside = run(AutoSupports {
        place: Place::Outside,
        ..own
    });
    assert_eq!(outside_of(&outside), outside_of(&same));
    assert_eq!(in_cavity(&outside).len(), 0);
    let _ = std::fs::remove_dir_all(directory);
}
