//! Junction control (M3, ALGORITHMS §6): automatic priority classes and signal phase plans on
//! T, X, Y, 5- and 6-way junctions; markings placement.

// Helpers outside `#[test]` fns assert by panicking, like the tests themselves.
#![allow(clippy::unwrap_used)]

use cb_geometry::Polyline2;
use cb_network::connectors::approach_lanes;
use cb_network::control::{
    ApproachControl, SignalState, auto_controls, phase_plan, rank, stub_phase_at,
};
use cb_network::junction::{Approach, Junction, build};
use cb_network::markings::{
    ApproachMarks, CROSSWALK_OFFSET_M, ISLAND_HALF_WIDTH_M, LineKind, STOP_GAP_M, markings,
};
use cb_network::{CrossSection, Profile, RoadKind, Variant};
use glam::Vec2;

fn arm(kind: RoadKind, deg: f32) -> Approach {
    let cs = CrossSection::with(kind, Variant::default());
    let (s, c) = cb_core::math::sin_cos(cb_core::math::to_radians(deg));
    let d = Vec2::new(c, s);
    Approach {
        centerline: Polyline2::new((0..=60).map(|k| d * (k as f32 * 2.0))),
        half_width_m: cs.half_width_m(),
        carriageway_half_m: cs.carriageway_half_m(),
        corner_radius_m: if matches!(kind, RoadKind::Avenue) {
            9.0
        } else {
            6.0
        },
        profile: Profile::flat(120.0, 0.0),
        reversed: false,
        lanes: approach_lanes(&cs, true),
    }
}

fn dirs(apps: &[Approach]) -> Vec<Vec2> {
    apps.iter().map(|a| a.centerline.tangent_at(0.0)).collect()
}

fn check_plan(name: &str, apps: &[Approach], j: &Junction) {
    let phases = phase_plan(&j.connectors, &j.zones, &dirs(apps));
    assert!(!phases.is_empty(), "{name}");
    // No conflicting greens: two green connectors from different approaches never share a zone.
    for (p, ph) in phases.iter().enumerate() {
        for z in &j.zones {
            let green: Vec<usize> = z
                .members
                .iter()
                .map(|m| m.0)
                .filter(|c| ph.connectors.contains(c))
                .collect();
            for a in &green {
                for b in &green {
                    assert!(
                        j.connectors[*a].from == j.connectors[*b].from,
                        "{name}: phase {p} greens conflicting {a} and {b}"
                    );
                }
            }
        }
    }
    // Every movement is green in some phase.
    for c in 0..j.connectors.len() {
        assert!(
            phases.iter().any(|p| p.connectors.contains(&c)),
            "{name}: connector {c} never green"
        );
    }
    assert!(phases.len() <= j.connectors.len());
    eprintln!(
        "{name}: {} connectors, {} phases",
        j.connectors.len(),
        phases.len()
    );
}

fn case(degs: &[f32]) -> (Vec<Approach>, Junction) {
    let apps: Vec<Approach> = degs.iter().map(|d| arm(RoadKind::Street, *d)).collect();
    let j = build(&apps).unwrap();
    (apps, j)
}

#[test]
fn light_plan_no_conflicting_greens_t() {
    let (a, j) = case(&[0.0, 90.0, 180.0]);
    check_plan("T", &a, &j);
}

#[test]
fn light_plan_no_conflicting_greens_x() {
    let (a, j) = case(&[0.0, 90.0, 180.0, 270.0]);
    check_plan("X", &a, &j);
    // Avenue × street with lights.
    let apps = vec![
        arm(RoadKind::Avenue, 0.0),
        arm(RoadKind::Street, 90.0),
        arm(RoadKind::Avenue, 180.0),
        arm(RoadKind::Street, 270.0),
    ];
    let j = build(&apps).unwrap();
    check_plan("avenue X", &apps, &j);
}

#[test]
fn light_plan_no_conflicting_greens_y() {
    let (a, j) = case(&[0.0, 35.0, 180.0]);
    check_plan("Y", &a, &j);
}

#[test]
fn light_plan_no_conflicting_greens_5way() {
    let (a, j) = case(&[0.0, 72.0, 144.0, 216.0, 288.0]);
    check_plan("5-way", &a, &j);
}

#[test]
fn light_plan_no_conflicting_greens_6way() {
    let (a, j) = case(&[0.0, 60.0, 120.0, 180.0, 240.0, 300.0]);
    check_plan("6-way", &a, &j);
}

#[test]
fn every_movement_green_in_some_phase() {
    for degs in [
        &[0.0, 90.0, 180.0][..],
        &[0.0, 90.0, 180.0, 270.0],
        &[0.0, 120.0, 240.0],
        &[0.0, 80.0, 180.0, 260.0],
    ] {
        let (a, j) = case(degs);
        let phases = phase_plan(&j.connectors, &j.zones, &dirs(&a));
        let covered: usize = (0..j.connectors.len())
            .filter(|c| phases.iter().any(|p| p.connectors.contains(c)))
            .count();
        assert_eq!(covered, j.connectors.len());
    }
}

#[test]
fn auto_controls_follow_hierarchy() {
    let d = [Vec2::X, Vec2::Y, -Vec2::X, -Vec2::Y];
    // Avenue through a street crossing: the avenue pair is major.
    let r = [
        rank(RoadKind::Avenue),
        rank(RoadKind::Street),
        rank(RoadKind::Avenue),
        rank(RoadKind::Street),
    ];
    use ApproachControl::{Priority as P, Yield as Y};
    assert_eq!(auto_controls(&r, &d), vec![P, Y, P, Y]);
    // A T where the street ends at an avenue: the avenue continues.
    assert_eq!(
        auto_controls(&[2, 1, 2], &[Vec2::X, Vec2::Y, -Vec2::X]),
        vec![P, Y, P]
    );
    // Equal ranks: the most opposite pair.
    let d3 = [Vec2::X, Vec2::new(-0.2, 1.0).normalize(), -Vec2::X];
    assert_eq!(auto_controls(&[1, 1, 1], &d3), vec![P, Y, P]);
    // A single major road continues into its best partner.
    assert_eq!(auto_controls(&[2, 1, 1], &d3), vec![P, Y, P]);
    // Bends: both priority.
    assert_eq!(auto_controls(&[1, 1], &[Vec2::X, Vec2::Y]), vec![P, P]);
}

#[test]
fn stub_clock_cycles_through_phases() {
    assert_eq!(stub_phase_at(0, 5.0, 0.0), None);
    assert_eq!(stub_phase_at(3, 0.0, 0.0), Some((0, SignalState::Green)));
    assert_eq!(stub_phase_at(3, 21.0, 0.0), Some((0, SignalState::Yellow)));
    assert_eq!(stub_phase_at(3, 23.5, 0.0), Some((0, SignalState::Red)));
    assert_eq!(stub_phase_at(3, 25.0, 0.0), Some((1, SignalState::Green)));
    assert_eq!(
        stub_phase_at(3, 3.0 * 24.5 + 1.0, 0.0),
        Some((0, SignalState::Green))
    );
}

#[test]
fn markings_placed_behind_cut_lines() {
    let (apps, j) = case(&[0.0, 90.0, 180.0]);
    let m = markings(&apps, &j, &[]);
    assert_eq!(m.crosswalks.len(), 3);
    assert_eq!(m.stop_lines.len(), 3);
    // One arriving lane per street approach, each with its movements painted.
    assert_eq!(m.arrows.len(), 3);
    for sl in &m.stop_lines {
        let a = &apps[sl.approach];
        let mid = (sl.ends[0] + sl.ends[1]) * 0.5;
        let (s, d) = (
            a.centerline.project(Vec2::new(mid.x, mid.z)).0,
            j.cuts[sl.approach],
        );
        assert!(s > d + CROSSWALK_OFFSET_M + STOP_GAP_M, "{s} vs cut {d}");
        // The stop line spans only the arriving half of the street (right of travel).
        let w = Vec2::new(sl.ends[1].x - sl.ends[0].x, sl.ends[1].z - sl.ends[0].z).length();
        assert!((w - 4.0).abs() < 0.1, "{w}");
    }
    let t_stem = m.arrows.iter().find(|a| a.approach == 1).unwrap();
    assert!(t_stem.turns.left && t_stem.turns.right && !t_stem.turns.straight);
    // No markings on bends.
    let (b, jb) = case(&[0.0, 150.0]);
    assert_eq!(markings(&b, &jb, &[]), Default::default());
}

#[test]
fn markings_follow_control_and_ring_roles() {
    let (apps, j) = case(&[0.0, 90.0, 180.0, 270.0]);
    let marks = [
        ApproachMarks::Free,
        ApproachMarks::GiveWay,
        ApproachMarks::Ring,
        ApproachMarks::RingEntry,
    ];
    let m = markings(&apps, &j, &marks);
    // Priority: crosswalk and arrows, no line. Ring: nothing at all.
    assert!(m.stop_lines.iter().all(|l| l.approach != 0));
    assert!(m.arrows.iter().any(|a| a.approach == 0));
    assert!(m.crosswalks.iter().all(|c| c.approach != 2));
    assert!(m.arrows.iter().all(|a| a.approach != 2 && a.approach != 3));
    let line = |i: usize| m.stop_lines.iter().find(|l| l.approach == i).unwrap();
    assert_eq!(line(1).kind, LineKind::GiveWay);
    assert_eq!(line(3).kind, LineKind::GiveWay);
    // Entry: the give-way line sits at the ring edge, the crosswalk is set back behind it and
    // the splitter island is cut for the pedestrian refuge.
    let a = &apps[3];
    let s_of = |p: glam::Vec3| a.centerline.project(Vec2::new(p.x, p.z)).0;
    let sl = s_of((line(3).ends[0] + line(3).ends[1]) * 0.5);
    let cw = m.crosswalks.iter().find(|c| c.approach == 3).unwrap();
    let sc = s_of((cw.ends[0] + cw.ends[1]) * 0.5);
    assert!(
        sl < j.cuts[3] + 1.0 && sc > sl + 4.0,
        "line {sl} crosswalk {sc}"
    );
    let islands: Vec<_> = m.islands.iter().filter(|i| i.approach == 3).collect();
    assert_eq!(islands.len(), 2);
    for is in &islands {
        for p in is.left.iter().chain(&is.right) {
            let (s, d) = a.centerline.project(Vec2::new(p.x, p.z));
            // Clear of the crosswalk band and of vehicles at both lane centres.
            assert!((s - sc).abs() > 1.5, "island on the crosswalk at {s}");
            assert!(d.abs() <= ISLAND_HALF_WIDTH_M + 1e-3, "{d}");
            for l in &a.lanes {
                assert!((l.offset_m - d).abs() > 0.9, "lane {} vs {d}", l.offset_m);
            }
        }
    }
}
