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
