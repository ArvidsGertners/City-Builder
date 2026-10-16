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
    println!(
        "probe=subdivision hash={:016x}",
        cb_geometry::subdivide::reference_families_hash()
    );
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
