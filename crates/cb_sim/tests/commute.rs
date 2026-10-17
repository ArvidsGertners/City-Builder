//! Macro commute model (ALGORITHMS §12, §16, ADR-0062): job matching uses routed estimates and
//! rejects commutes above 300 sim s, the daily assignment publishes commutes and a departure
//! histogram that follows the shift data, day costs rise with assigned volume, and one-way traps
//! are flagged (TESTING §3).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};
use cb_geometry::CubicBezier2;
use cb_network::RoadKind;
use cb_sim::catalog::Catalog;
use cb_sim::citizens::{Capacities, Population};
use cb_sim::citygen::{self, GridSpec};
use cb_sim::commands::{BuildRoad, Endpoint, PlayerCommand};
use cb_sim::ids::Ids;
use cb_sim::roads::RoadNode;
use cb_sim::testing::Scenario;
use cb_sim::traffic::{self, Traffic};
use glam::Vec2;

fn road(s: &mut Scenario, kind: RoadKind, a: Vec2, b: Vec2) {
    let r = s.command(PlayerCommand::BuildRoad(BuildRoad {
        kind,
        curve: CubicBezier2::straight(a, b),
        start: Endpoint::Free,
        end: Endpoint::Free,
    }));
    assert!(r.outcome.is_ok(), "{:?}", r.outcome);
}

fn town(seed: u64) -> Scenario {
    let mut s = Scenario::new(seed);
    citygen::grid(
        s.world_mut(),
        &GridSpec {
            blocks: (4, 3),
            mixed: true,
            ..GridSpec::default()
        },
    );
    s
}

#[test]
fn commute_estimates_routed_and_published_daily() {
    let mut s = town(3);
    // Day 0's slices pass before the first households arrive; day 1 is the first full day.
    s.run_ticks(2 * TICKS_PER_GAME_DAY + TICKS_PER_GAME_HOUR);
    let max = s.world().resource::<Catalog>().citizens().commute.max_sim_s;
    let pop = s.world().resource::<Population>();
    let mut employed = 0;
    for (id, c) in pop.citizens() {
        if c.job().is_some() {
            employed += 1;
            let sim_s = c
                .commute_sim_s()
                .unwrap_or_else(|| panic!("{id} has an estimate"));
            assert!(f32::from(sim_s) <= 2.0 * max, "{id}: {sim_s} sim s");
            assert!(c.shift_start_min() < 1440);
        }
    }
    assert!(employed > 20, "{employed} employed");
    let st = *s.world().resource::<Traffic>().stats();
    assert!(st.commuters > 0, "{st:?}");
    assert_eq!(st.no_route, 0, "{st:?}");
    assert_eq!(st.departures.iter().sum::<u32>(), st.commuters, "{st:?}");
    assert!(
        (0.5..=30.0).contains(&st.p50_perceived_min),
        "p50 {}",
        st.p50_perceived_min
    );
    assert!(st.p95_perceived_min >= st.p50_perceived_min);
}

#[test]
fn departures_follow_shift_starts() {
    let mut s = town(4);
    s.run_ticks(2 * TICKS_PER_GAME_DAY + 10);
    let st = *s.world().resource::<Traffic>().stats();
    let peak = (0..st.departures.len())
        .max_by_key(|b| (st.departures[*b], std::cmp::Reverse(*b)))
        .unwrap();
    // Industry N(07:30, 45), commerce N(08:45, 75) minus a few minutes' drive and the slack.
    assert!(
        (10..=17).contains(&peak),
        "peak bin {peak} {:?}",
        st.departures
    );
    let morning: u32 = st.departures[10..24].iter().sum();
    assert!(
        morning * 10 >= st.commuters * 6,
        "{morning} of {} leave 05:00–12:00",
        st.commuters
    );
}

#[test]
fn day_costs_rise_with_assigned_volume() {
    let mut s = town(5);
    s.run_ticks(2 * TICKS_PER_GAME_DAY + 10);
    let t = s.world().resource::<Traffic>();
    let g = t.graph();
    let spec = s.world().resource::<Catalog>().citizens().commute;
    let mut loaded = 0;
    for l in 0..g.link_count() as u32 {
        let day = t.day_ms(l).unwrap();
        assert!(day >= g.free_ms(l), "never below free flow");
        if t.peak_volume_per_sim_h(l, &spec) > 0.0 {
            loaded += 1;
            let free = cb_traffic::flow::bpr(1.0, t.peak_volume_per_sim_h(l, &spec), 1.0e9);
            assert!(free >= 1.0);
        }
        if t.peak_volume_per_sim_h(l, &spec) > 0.2 * g.link(l).capacity_per_sim_h {
            assert!(day > g.free_ms(l), "link {l} congested costs more");
        }
    }
    assert!(loaded > 5, "{loaded} loaded links");
    // Noise sees the flow-model volume.
    let ids = s.world().resource::<Ids>();
    let busiest = ids
        .segments
        .iter()
        .map(|(sid, _)| t.segment_volume_per_sim_h(sid, spec.trip_scale))
        .fold(0.0f32, f32::max);
    assert!(busiest > 0.0);
}

#[test]
fn job_matching_rejects_commutes_over_300_sim_s() {
    let mut s = Scenario::new(9);
    // Homes only in the west, jobs (and homes) in the east, 3.3 km of dirt path between them:
    // ≈ 390 sim s at 30 km/h, over the 300 sim s limit.
    citygen::grid(
        s.world_mut(),
        &GridSpec {
            center: Vec2::new(-1800.0, 0.0),
            blocks: (3, 2),
            mixed: false,
            ..GridSpec::default()
        },
    );
    citygen::grid(
        s.world_mut(),
        &GridSpec {
            center: Vec2::new(1800.0, 0.0),
            blocks: (3, 2),
            mixed: true,
            town_hall: false,
            ..GridSpec::default()
        },
    );
    road(
        &mut s,
        RoadKind::Dirt,
        Vec2::new(-1635.0, 0.0),
        Vec2::new(1635.0, 0.0),
    );
    s.run_ticks(10 * TICKS_PER_GAME_HOUR);
    let t = s.world().resource::<Traffic>();
    assert!(
        (0..t.graph().node_count() as u32).all(|v| !t.trapped(v)),
        "the towns are connected: only the distance rejects"
    );
    let pop = s.world().resource::<Population>();
    let cap = s.world().resource::<Capacities>();
    let mut west = 0;
    let mut east_employed = 0;
    for (_, c) in pop.citizens() {
        let home = pop.household(c.household()).unwrap().home();
        let x = cap.pos(home).unwrap().x;
        if x < 0.0 && c.in_labour_force() {
            west += 1;
            assert!(c.job().is_none(), "west residents cannot reach the jobs");
        } else if c.job().is_some() {
            east_employed += 1;
        }
    }
    assert!(west > 0, "someone lives in the west");
    assert!(east_employed > 0, "east residents work");
}

#[test]
fn one_way_trap_flagged_no_route() {
    let mut s = Scenario::new(1);
    road(
        &mut s,
        RoadKind::Street,
        Vec2::new(-200.0, 0.0),
        Vec2::new(200.0, 0.0),
    );
    road(
        &mut s,
        RoadKind::OneWayStreet,
        Vec2::new(0.0, 0.0),
        Vec2::new(0.0, 200.0),
    );
    s.run_ticks(1);
    let ids = s.world().resource::<Ids>();
    let t = s.world().resource::<Traffic>();
    let tip = ids
        .nodes
        .iter()
        .find(|(_, e)| {
            let p = s.world().get::<RoadNode>(*e).unwrap().pos();
            (p.x.abs() + (p.z - 200.0).abs()) < 1.0
        })
        .map(|(id, _)| id)
        .unwrap();
    let mut trapped = Vec::new();
    for (id, _) in ids.nodes.iter() {
        let idx = (0..t.graph().node_count() as u32)
            .find(|v| {
                let p = t.graph().node_pos(*v);
                let q = s
                    .world()
                    .get::<RoadNode>(ids.nodes.get(id).unwrap())
                    .unwrap()
                    .pos();
                p.distance(Vec2::new(q.x, q.z)) < 0.01
            })
            .unwrap();
        if t.trapped(idx) {
            trapped.push(id);
        }
    }
    assert_eq!(
        trapped,
        vec![tip],
        "only the dead-end tip of the one-way is a trap"
    );
    assert!(traffic::building_no_route(
        s.world(),
        cb_core::ids::BuildingId::try_new(999).unwrap()
    ));
}
