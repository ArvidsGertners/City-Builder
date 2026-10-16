//! Junction geometry with connectors (ADR-0063; the S-J metrics as tests): canonical cases,
//! 10 000 random junctions, and surface continuity on an 8 % grade.

use cb_core::rng::Pcg32;
use cb_geometry::Polyline2;
use cb_network::connectors::{ConnectorKind, MAX_TURN_DEG, approach_lanes, polylines_cross};
use cb_network::junction::{Approach, CONNECTOR_OUTSIDE_TOL_M, Junction, JunctionError, build};
use cb_network::{CrossSection, LaneKind, Profile, RoadKind, Variant};
use glam::{Vec2, Vec3};

fn corner_radius(kind: RoadKind) -> f32 {
    match kind {
        RoadKind::Dirt => 4.0,
        RoadKind::Avenue | RoadKind::OneWayAvenue => 9.0,
        RoadKind::TramTrack => 20.0,
        _ => 6.0,
    }
}

/// An approach leaving the node at `deg`, with constant `curvature` (1/m, left positive),
/// `len` long, rising `grade` per metre away from the node (with a 4 % landing over the first
/// `landing` metres). `incoming_one_way`: a one-way road whose lanes all drive toward the node.
#[allow(clippy::too_many_arguments)]
fn arm(
    kind: RoadKind,
    v: Variant,
    deg: f32,
    curvature: f32,
    len: f32,
    grade: f32,
    landing: f32,
    incoming_one_way: bool,
) -> Approach {
    let mut pts = Vec::new();
    let a0 = cb_core::math::to_radians(deg);
    let n = (len / 1.0).ceil() as usize;
    let mut p = Vec2::ZERO;
    pts.push(p);
    for k in 0..n {
        let s = (k as f32 + 0.5) * len / n as f32;
        let (sn, cs) = cb_core::math::sin_cos(a0 + curvature * s);
        p += Vec2::new(cs, sn) * (len / n as f32);
        pts.push(p);
    }
    let cs = CrossSection::with(kind, v);
    let g_land = grade.clamp(-0.04, 0.04);
    let hs: Vec<f32> = (0..=n)
        .map(|k| {
            let s = k as f32 * len / n as f32;
            if s <= landing {
                g_land * s
            } else {
                g_land * landing + grade * (s - landing)
            }
        })
        .collect();
    let ss: Vec<f32> = (0..=n).map(|k| k as f32 * len / n as f32).collect();
    // One-way roads: the curve runs toward the node for incoming arms (the approach is then
    // "at the end" of its segment), away from it otherwise.
    let one_way = matches!(kind, RoadKind::OneWayStreet | RoadKind::OneWayAvenue);
    let at_start = !(one_way && incoming_one_way);
    Approach {
        centerline: Polyline2::new(pts),
        half_width_m: cs.half_width_m(),
        carriageway_half_m: cs.carriageway_half_m(),
        corner_radius_m: corner_radius(kind),
        profile: Profile::new(ss, hs),
        reversed: false,
        lanes: approach_lanes(&cs, at_start),
    }
}

fn street(deg: f32) -> Approach {
    arm(
        RoadKind::Street,
        Variant::default(),
        deg,
        0.0,
        120.0,
        0.0,
        0.0,
        false,
    )
}

fn flat(p: &[Vec3]) -> Vec<Vec2> {
    p.iter().map(|v| Vec2::new(v.x, v.z)).collect()
}

fn inside_or_near(p: Vec2, poly: &[Vec2]) -> bool {
    let mut c = false;
    let n = poly.len();
    for k in 0..n {
        let (a, b) = (poly[k], poly[(k + n - 1) % n]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            c = !c;
        }
    }
    c || (0..n).any(|k| {
        let (a, b) = (poly[k], poly[(k + 1) % n]);
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-9)).clamp(0.0, 1.0);
        (p - (a + ab * t)).length() <= CONNECTOR_OUTSIDE_TOL_M + 1e-3
    })
}

fn tangent(a: &Approach, s: f32) -> Vec2 {
    a.centerline.tangent_at(s)
}

/// Every ADR-0063 validity rule; returns a description of the first violation.
fn violations(apps: &[Approach], j: &Junction) -> Option<String> {
    let n = apps.len();
    let inner = flat(&j.inner);
    if n >= 3 && cb_geometry::polygon::signed_area(&inner) <= 0.0 {
        return Some("inner polygon not CCW".into());
    }
    for (ci, c) in j.connectors.iter().enumerate() {
        let limit = match c.kind {
            ConnectorKind::Tram => 1.0 / 20.0,
            ConnectorKind::Road => {
                // Cars on a shared car/tram lane turn with the car limit.
                let bus = apps[c.from]
                    .lanes
                    .iter()
                    .any(|l| l.incoming && l.index == c.from_lane && l.kind == LaneKind::Bus);
                let kind = if bus { LaneKind::Bus } else { LaneKind::Car };
                1.0 / kind.min_connector_radius_m()
            }
        };
        // Re-sourced alternatives may be sharper than the limit by construction; the rule is on
        // the generated movement set, so allow 2 % for numeric sampling.
        if c.max_curvature > limit * 1.02 {
            return Some(format!(
                "connector {ci} curvature {:.3} > {limit:.3}",
                c.max_curvature
            ));
        }
        if n >= 3
            && !c
                .points
                .iter()
                .all(|p| inside_or_near(Vec2::new(p.x, p.z), &inner))
        {
            return Some(format!("connector {ci} leaves the carriageway"));
        }
        if c.deflection_deg.abs() > MAX_TURN_DEG + 1e-3 {
            return Some(format!("connector {ci} deflects {:.0}°", c.deflection_deg));
        }
    }
    for x in 0..j.connectors.len() {
        for y in x + 1..j.connectors.len() {
            let (a, b) = (&j.connectors[x], &j.connectors[y]);
            if a.from == b.from
                && a.kind == b.kind
                && a.from_lane != b.from_lane
                && polylines_cross(&flat(&a.points), &flat(&b.points))
            {
                return Some(format!("connectors {x} and {y} from different lanes cross"));
            }
        }
    }
    None
}

/// Every allowed movement (both approaches carry suitable lanes, deflection ≤ 135°) has a
/// connector.
fn missing_movements(apps: &[Approach], j: &Junction) -> Vec<(usize, usize, ConnectorKind)> {
    let mut missing = Vec::new();
    for (i, a) in apps.iter().enumerate() {
        for (k, b) in apps.iter().enumerate() {
            if i == k {
                continue;
            }
            let d_in = -tangent(a, j.cuts[i]);
            let d_out = tangent(b, j.cuts[k]);
            let defl = cb_core::math::to_degrees(cb_core::math::atan2(
                d_in.perp_dot(d_out),
                d_in.dot(d_out),
            ));
            if defl.abs() > MAX_TURN_DEG {
                continue;
            }
            for kind in [ConnectorKind::Road, ConnectorKind::Tram] {
                let ok = |l: &cb_network::connectors::ApproachLane| match kind {
                    ConnectorKind::Road => l.kind != LaneKind::Tram,
                    ConnectorKind::Tram => l.kind.trams(),
                };
                let has_in = a.lanes.iter().any(|l| l.incoming && ok(l));
                let has_out = b.lanes.iter().any(|l| !l.incoming && ok(l));
                if has_in
                    && has_out
                    && !j
                        .connectors
                        .iter()
                        .any(|c| c.from == i && c.to == k && c.kind == kind)
                {
                    missing.push((i, k, kind));
                }
            }
        }
    }
    missing
}

#[test]
fn junction_connectors_cover_all_allowed_movements() {
    let none = Variant::default();
    let tram = Variant {
        bus_lanes: false,
        tram_tracks: true,
    };
    let bus = Variant {
        bus_lanes: true,
        tram_tracks: false,
    };
    let av = |deg, v| arm(RoadKind::Avenue, v, deg, 0.0, 160.0, 0.0, 0.0, false);
    let cases: Vec<(&str, Vec<Approach>)> = vec![
        ("T", vec![street(0.0), street(90.0), street(180.0)]),
        (
            "X",
            vec![street(0.0), street(90.0), street(180.0), street(270.0)],
        ),
        ("Y30", vec![street(0.0), street(30.0), street(180.0)]),
        (
            "5-way",
            vec![
                street(0.0),
                street(72.0),
                street(144.0),
                street(216.0),
                street(288.0),
            ],
        ),
        (
            "6-way",
            vec![
                street(0.0),
                street(60.0),
                street(120.0),
                street(180.0),
                street(240.0),
                street(300.0),
            ],
        ),
        (
            "avenue x street",
            vec![av(0.0, none), street(90.0), av(180.0, none), street(270.0)],
        ),
        (
            "tram avenue T",
            vec![av(0.0, tram), street(90.0), av(180.0, tram)],
        ),
        (
            "bus avenue X",
            vec![av(0.0, bus), av(90.0, bus), av(180.0, bus), av(270.0, bus)],
        ),
        (
            "one-way pair",
            vec![
                arm(
                    RoadKind::OneWayStreet,
                    none,
                    0.0,
                    0.0,
                    120.0,
                    0.0,
                    0.0,
                    true,
                ),
                street(90.0),
                arm(
                    RoadKind::OneWayStreet,
                    none,
                    180.0,
                    0.0,
                    120.0,
                    0.0,
                    0.0,
                    false,
                ),
                street(270.0),
            ],
        ),
        ("bend", vec![street(0.0), street(150.0)]),
    ];
    for (name, apps) in cases {
        let j = build(&apps).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert!(
            violations(&apps, &j).is_none(),
            "{name}: {:?}",
            violations(&apps, &j)
        );
        let missing = missing_movements(&apps, &j);
        assert!(missing.is_empty(), "{name}: missing {missing:?}");
        if apps.len() >= 3 {
            assert!(!j.zones.is_empty(), "{name}: no conflict zones");
        }
        // Zone members reference real connectors with arc lengths on them.
        for z in &j.zones {
            assert!(z.members.len() >= 2);
            for (c, s) in &z.members {
                assert!(
                    *s >= -1e-3 && *s <= j.connectors[*c].length_m + 1e-3,
                    "{name}"
                );
            }
        }
    }
    // A T: 6 car movements; a 1+1 street X: 12.
    let t = build(&[street(0.0), street(90.0), street(180.0)]).unwrap();
    assert_eq!(t.connectors.len(), 6);
    let x = build(&[street(0.0), street(90.0), street(180.0), street(270.0)]).unwrap();
    assert_eq!(x.connectors.len(), 12);
}

#[test]
fn junction_random_10k_valid_or_typed_error() {
    let kinds = [
        RoadKind::Dirt,
        RoadKind::Street,
        RoadKind::Street,
        RoadKind::Street,
        RoadKind::OneWayStreet,
        RoadKind::Avenue,
        RoadKind::OneWayAvenue,
    ];
    let mut rng = Pcg32::new(0x5EED_0063, 1);
    let (mut ok, mut errs) = (0usize, std::collections::BTreeMap::<String, usize>::new());
    let cases = if cfg!(debug_assertions) {
        2_000
    } else {
        10_000
    };
    for case in 0..cases {
        let n = 2 + (rng.next_u32() % 5) as usize;
        // Half the cases spread the arms (gaps 0.5–1.5 × even), half place them anywhere (many
        // of those are rejected as too acute, which is part of what is tested).
        let mut angles: Vec<f32> = if case % 2 == 0 {
            let gaps: Vec<f32> = (0..n).map(|_| rng.range_f32(0.5, 1.5)).collect();
            let total: f32 = gaps.iter().sum();
            let a0 = rng.range_f32(0.0, 360.0);
            let mut acc = 0.0;
            gaps.iter()
                .map(|g| {
                    let a = (a0 + acc) % 360.0;
                    acc += g / total * 360.0;
                    a
                })
                .collect()
        } else {
            (0..n).map(|_| rng.range_f32(0.0, 360.0)).collect()
        };
        angles.sort_by(f32::total_cmp);
        let apps: Vec<Approach> = angles
            .iter()
            .map(|&deg| {
                let kind = kinds[(rng.next_u32() as usize) % kinds.len()];
                let curv = rng.range_f32(-1.0 / 60.0, 1.0 / 60.0);
                let len = rng.range_f32(40.0, 200.0);
                let grade = rng.range_f32(-0.08, 0.08);
                arm(
                    kind,
                    Variant::default(),
                    deg,
                    curv,
                    len,
                    grade,
                    30.0,
                    rng.next_u32().is_multiple_of(2),
                )
            })
            .collect();
        match build(&apps) {
            Ok(j) => {
                ok += 1;
                if let Some(v) = violations(&apps, &j) {
                    panic!("case {case} (angles {angles:?}): {v}");
                }
                let missing = missing_movements(&apps, &j);
                assert!(missing.is_empty(), "case {case}: missing {missing:?}");
            }
            Err(e) => {
                *errs.entry(format!("{e:?}")).or_default() += 1;
                // Typed errors only (no panics, no silent garbage).
                assert!(matches!(
                    e,
                    JunctionError::AngleTooSmall
                        | JunctionError::TooClose
                        | JunctionError::PolygonInvalid
                        | JunctionError::DegreeTooHigh
                        | JunctionError::ConnectorTooSharp
                ));
            }
        }
    }
    eprintln!("random junctions: {ok} valid of {cases}; errors {errs:?}");
    assert!(ok * 3 > cases, "too few valid junctions: {ok} of {cases}");
}

#[test]
fn junction_surface_continuous_8pct() {
    // A T on an 8 % slope: every arm climbs/falls 8 % with the 4 % landing over cut + 4 m.
    let landing = 26.0;
    let apps = vec![
        arm(
            RoadKind::Street,
            Variant::default(),
            0.0,
            0.0,
            140.0,
            0.08,
            landing,
            false,
        ),
        arm(
            RoadKind::Street,
            Variant::default(),
            90.0,
            0.0,
            140.0,
            0.0,
            landing,
            false,
        ),
        arm(
            RoadKind::Street,
            Variant::default(),
            180.0,
            0.0,
            140.0,
            -0.08,
            landing,
            false,
        ),
    ];
    let j = build(&apps).unwrap();
    for (i, a) in apps.iter().enumerate() {
        assert!(
            j.cuts[i] + 4.0 <= landing,
            "landing too short for cut {}",
            j.cuts[i]
        );
        let h = a.height(j.cuts[i]);
        let cut_pt = a.centerline.point_at(j.cuts[i]);
        // Polygon vertices on this approach's cut line carry its surface height (step 0).
        let on_cut: Vec<&Vec3> = j
            .inner
            .iter()
            .filter(|v| {
                let d = Vec2::new(v.x, v.z) - cut_pt;
                d.dot(tangent(a, j.cuts[i])).abs() < 1e-3
            })
            .collect();
        assert!(!on_cut.is_empty());
        for v in on_cut {
            assert!((v.y - h).abs() <= 0.02, "step {} at approach {i}", v.y - h);
        }
        // Connectors leave and enter at the lane surface heights.
        for c in j.connectors.iter().filter(|c| c.from == i) {
            assert!((c.points[0].y - h).abs() <= 0.02);
        }
        for c in j.connectors.iter().filter(|c| c.to == i) {
            assert!((c.points.last().unwrap().y - h).abs() <= 0.02);
        }
    }
    // Connector heights change smoothly: no sample-to-sample grade above the 8 % road grade.
    let mut worst = 0.0f32;
    for c in &j.connectors {
        for w in c.points.windows(2) {
            let run = Vec2::new(w[1].x - w[0].x, w[1].z - w[0].z)
                .length()
                .max(1e-3);
            worst = worst.max((w[1].y - w[0].y).abs() / run);
        }
    }
    eprintln!("worst connector grade {worst:.3}");
    assert!(worst <= 0.08, "connector grade {worst}");
}

#[test]
fn carriageway_triangles_tile_the_polygon() {
    use cb_network::junction::{carriageway_triangles, mvc_height};
    // Flat X (single fan) and a T on an 8 % slope (rings): triangles cover the carriageway
    // exactly once and the surface heights are the boundary's mean-value interpolant.
    let flat_x = vec![street(0.0), street(90.0), street(180.0), street(270.0)];
    let sloped_t = vec![
        arm(
            RoadKind::Street,
            Variant::default(),
            0.0,
            0.0,
            140.0,
            0.08,
            26.0,
            false,
        ),
        arm(
            RoadKind::Avenue,
            Variant::default(),
            90.0,
            0.0,
            140.0,
            0.0,
            26.0,
            false,
        ),
        arm(
            RoadKind::Street,
            Variant::default(),
            180.0,
            0.0,
            140.0,
            -0.08,
            26.0,
            false,
        ),
    ];
    for (name, apps) in [("flat X", flat_x), ("sloped T", sloped_t)] {
        let j = build(&apps).unwrap();
        let tris = carriageway_triangles(&j.inner);
        let poly = flat(&j.inner);
        let area = cb_geometry::polygon::area(&poly);
        let sum: f32 = tris
            .iter()
            .map(|t| {
                let (a, b, c) = (
                    Vec2::new(t[0].x, t[0].z),
                    Vec2::new(t[1].x, t[1].z),
                    Vec2::new(t[2].x, t[2].z),
                );
                0.5 * (b - a).perp_dot(c - a).abs()
            })
            .sum();
        assert!((sum - area).abs() / area < 0.002, "{name}: {sum} vs {area}");
        for t in &tris {
            for v in t {
                let p = Vec2::new(v.x, v.z);
                assert!(inside_or_near(p, &poly), "{name}: vertex outside");
                assert!((v.y - mvc_height(&j.inner, p)).abs() < 1e-3, "{name}");
            }
        }
        eprintln!("{name}: {} triangles", tris.len());
    }
}
