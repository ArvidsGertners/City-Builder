//! Prints state hashes for fixed seeds so CI can compare them across operating systems
//! (TESTING §5.6). The scenario grows each milestone; output is one `key=value` line per probe.

use cb_core::time::TICKS_PER_GAME_DAY;
use cb_sim::testing::Scenario;

const SEEDS: [u64; 5] = [1, 2, 3, 0xC17E, 0xDEAD_BEEF];

fn main() {
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
