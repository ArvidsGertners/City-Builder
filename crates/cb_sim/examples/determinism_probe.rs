//! Prints state hashes for fixed seeds so CI can compare them across operating systems
//! (TESTING §5.6). The scenario grows each milestone; output is one `key=value` line per probe.

use cb_core::time::TICKS_PER_GAME_DAY;
use cb_sim::testing::Scenario;

const SEEDS: [u64; 5] = [1, 2, 3, 0xC17E, 0xDEAD_BEEF];

fn main() {
    // terrain_hash_cross_os (M2): world generation must be bit-identical on every OS.
    for seed in &SEEDS[..3] {
        match cb_terrain::generate(&cb_terrain::WorldGenParams::default(), *seed) {
            Ok(w) => println!("probe=terrain seed={seed} hash={:016x}", w.content_hash()),
            Err(e) => println!("probe=terrain seed={seed} error={e}"),
        }
    }
    println!("probe=subdivision hash={:016x}", subdivision_hash());
    // M2 scene (roads, grading, junctions, plots, lots, houses on generated terrain).
    let mut s = Scenario::with_terrain(1234, cb_sim::terrain::TerrainSetup::Generate);
    let site = glam::Vec2::new(19.2, -1228.8);
    let grid = cb_sim::citygen::grid(
        s.world_mut(),
        &cb_sim::citygen::GridSpec {
            center: site,
            blocks: (4, 3),
            ..Default::default()
        },
    );
    let f = cb_sim::citygen::m2_features(s.world_mut(), site, glam::Vec2::new(220.0, 120.0));
    println!(
        "probe=m2_scene seed=1234 buildings={} hash={:016x}",
        grid.buildings + f.report.buildings,
        s.state_hash()
    );
    for seed in SEEDS {
        let mut s = Scenario::new(seed);
        s.run_ticks(2 * TICKS_PER_GAME_DAY);
        println!(
            "probe=clock seed={seed} ticks={} hash={:016x}",
            s.ticks(),
            s.state_hash()
        );
    }
}

/// subdivision_hash_cross_os (M2): one plot per S-P frontage family, subdivided and fitted;
/// hashes the bit patterns of every lot and footprint corner (libm trig, i_overlay snapping).
fn subdivision_hash() -> u64 {
    use cb_geometry::frontage::{Frontage, Prim};
    use cb_geometry::rect_fit::{FitParams, RectSpec, fit_lots};
    use cb_geometry::subdivide::{ZoneParams, subdivide};
    let d = f64::to_radians;
    let plots = [
        vec![Prim::Line(120.0)],
        vec![Prim::Arc(60.0, -d(110.0))],
        vec![Prim::Arc(50.0, d(120.0))],
        vec![Prim::Arc(80.0, d(50.0)), Prim::Arc(50.0, -d(70.0))],
        vec![Prim::Line(70.0), Prim::Arc(8.0, d(120.0)), Prim::Line(70.0)],
        vec![
            Prim::Line(60.0),
            Prim::Arc(10.0, -d(35.0)),
            Prim::Line(60.0),
        ],
    ];
    let zp = ZoneParams::RESIDENTIAL;
    let fit = FitParams {
        setback: zp.setback,
        rect: Some(RectSpec::HOUSE),
    };
    let mut bits: Vec<u64> = Vec::new();
    for (k, prims) in plots.iter().enumerate() {
        let depth = 30.0;
        let Some(f) = Frontage::from_prims(prims, depth) else {
            continue;
        };
        let Some(plot) = f.plot_polygon(depth, 12.0) else {
            continue;
        };
        let out = subdivide(&f, &plot, &zp, depth, 0, 0x5EED_0000 + k as u64, None);
        for l in &out.lots {
            bits.extend(l.poly.iter().flat_map(|v| [v.x.to_bits(), v.y.to_bits()]));
        }
        for fp in fit_lots(&out.lots, &f, &fit)
            .iter()
            .filter_map(|x| x.footprint)
        {
            bits.extend(fp.iter().flat_map(|v| [v.x.to_bits(), v.y.to_bits()]));
        }
    }
    cb_core::hash::stable_hash(&bits)
}
