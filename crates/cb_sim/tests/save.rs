//! Save core v0 (ADR-0044): a saved game loads with the same state hash, continues exactly
//! like the game that was never saved, rejects corrupt or inconsistent files with typed errors
//! and never panics on malformed input; the migration framework and the struct-map encoding
//! with defaults work (TESTING §3).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use cb_core::rng::Pcg32;
use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR};
use cb_sim::citizens::Population;
use cb_sim::citygen::{self, GridSpec};
use cb_sim::save::format::{self, FORMAT_VERSION, SaveError};
use cb_sim::save::migrate::{self, Migration};
use cb_sim::testing::Scenario;
use cb_sim::traffic::Traffic;
use cb_sim::{TerrainSetup, state_hash};
use rmpv::Value;

fn town(seed: u64, terrain: TerrainSetup) -> Scenario {
    let mut s = Scenario::with_terrain(seed, terrain);
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

fn flat() -> TerrainSetup {
    TerrainSetup::Flat {
        samples: 513,
        spacing_m: 4.0,
        h: 10.0,
    }
}

fn reencode(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    rmpv::encode::write_value(&mut out, v).unwrap();
    out
}

#[test]
fn save_roundtrip_hash_equal() {
    let mut s = town(3, flat());
    s.run_ticks(TICKS_PER_GAME_DAY + 5 * TICKS_PER_GAME_HOUR + 333);
    let bytes = s.save();
    let header = format::read_header(&bytes).unwrap();
    assert_eq!(header.state_hash, s.state_hash());
    assert_eq!(header.format_version, FORMAT_VERSION);
    assert!(header.population > 50, "{header:?}");
    let loaded = Scenario::load(&bytes).unwrap();
    assert_eq!(loaded.state_hash(), header.state_hash, "hash after load");
    assert_eq!(state_hash(loaded.world()), state_hash(s.world()));
    // Saving the loaded game again gives the same body.
    let again = loaded.save();
    let (_, _, b0) = format::split(&bytes).unwrap();
    let (_, _, b1) = format::split(&again).unwrap();
    assert!(
        b0 == b1,
        "re-saved body differs ({} vs {} bytes)",
        b0.len(),
        b1.len()
    );
}

#[test]
fn save_load_continue_equals_continuous() {
    // Saved in the middle of the hourly field slices and the daily commute assignment.
    let mut a = town(5, flat());
    a.run_ticks(TICKS_PER_GAME_DAY + 7 * TICKS_PER_GAME_HOUR + 317);
    let bytes = a.save();
    let mut b = Scenario::load(&bytes).unwrap();
    for step in 0..6 {
        a.run_ticks(4 * TICKS_PER_GAME_HOUR + 1);
        b.run_ticks(4 * TICKS_PER_GAME_HOUR + 1);
        assert_eq!(a.state_hash(), b.state_hash(), "diverged in step {step}");
    }
    let pa = a.world().resource::<Population>();
    let pb = b.world().resource::<Population>();
    assert_eq!(
        format!("{:?}", pa.citizens().collect::<Vec<_>>()),
        format!("{:?}", pb.citizens().collect::<Vec<_>>())
    );
    assert_eq!(
        a.world().resource::<Traffic>().stats(),
        b.world().resource::<Traffic>().stats()
    );
    assert_eq!(
        a.world().resource::<cb_sim::fields::Fields>(),
        b.world().resource::<cb_sim::fields::Fields>()
    );
}

#[test]
fn save_corrupt_returns_error() {
    let mut s = town(7, TerrainSetup::None);
    s.run_ticks(3 * TICKS_PER_GAME_HOUR);
    let bytes = s.save();
    let mut flipped = bytes.clone();
    let mid = flipped.len() / 2;
    flipped[mid] ^= 0x40;
    assert_eq!(format::decode(&flipped).err(), Some(SaveError::Checksum));
    assert!(matches!(
        format::decode(&bytes[..bytes.len() / 3]).err(),
        Some(SaveError::Checksum | SaveError::Truncated)
    ));
    assert_eq!(
        format::decode(&bytes[..10]).err(),
        Some(SaveError::Truncated)
    );
    assert_eq!(
        format::decode(b"not a save file at all, really").err(),
        Some(SaveError::BadMagic)
    );
    let (_, h, b) = format::split(&bytes).unwrap();
    let newer = format::assemble(FORMAT_VERSION + 1, h, b);
    assert_eq!(
        format::decode(&newer).err(),
        Some(SaveError::UnsupportedVersion(FORMAT_VERSION + 1))
    );
    // A dangling reference: the first household lives in a building that does not exist.
    let mut v: Value = rmp_serde::from_slice(b).unwrap();
    let pop = migrate::field_mut(&mut v, "population").unwrap();
    let hh = migrate::field_mut(pop, "households").unwrap();
    let Value::Map(entries) = hh else {
        panic!("households map")
    };
    let first = &mut entries[0].1;
    *migrate::field_mut(first, "home").unwrap() = Value::from(999_999u32);
    let bad = format::assemble(FORMAT_VERSION, h, &reencode(&v));
    let err = format::decode(&bad).err();
    assert!(
        matches!(err, Some(SaveError::Invalid(ref m)) if m.contains("lives in missing")),
        "{err:?}"
    );
    // Loading into a world that already has objects is refused.
    assert!(matches!(
        cb_sim::save::load(s.world_mut(), &bytes),
        Err(SaveError::Invalid(_))
    ));
}

#[test]
fn save_fuzz_no_panic() {
    let mut s = town(9, TerrainSetup::None);
    s.run_ticks(2 * TICKS_PER_GAME_HOUR);
    let bytes = s.save();
    let (_, h, b) = format::split(&bytes).unwrap();
    let mut rng = Pcg32::from_seed(77);
    let mut errors = 0;
    for round in 0..400 {
        let mut body = b.to_vec();
        match round % 4 {
            0 => {
                for _ in 0..1 + rng.below(8) {
                    let i = rng.below(body.len() as u32) as usize;
                    body[i] = rng.next_u32() as u8;
                }
            }
            1 => body.truncate(rng.below(body.len() as u32) as usize),
            2 => {
                let i = rng.below(body.len() as u32) as usize;
                let n = (rng.below(64) as usize).min(body.len() - i);
                body.drain(i..i + n);
            }
            _ => {
                let i = rng.below(body.len() as u32) as usize;
                body.insert(i, rng.next_u32() as u8);
            }
        }
        // Valid framing and checksum, so the decoder and validator see the mutation.
        let file = format::assemble(FORMAT_VERSION, h, &body);
        if format::decode(&file).is_err() {
            errors += 1;
        }
    }
    assert!(errors > 200, "most mutations are rejected ({errors})");
}

#[test]
fn migration_v0_to_v1_synthetic() {
    // A synthetic v1 renames `tick` to `ticks` and adds a field.
    let steps = [Migration {
        from: 0,
        step: |v| {
            if !migrate::rename_field(v, "tick", "ticks") {
                return Err("no tick".into());
            }
            let Value::Map(m) = v else {
                return Err("not a map".into());
            };
            m.push((Value::from("weather"), Value::from("clear")));
            Ok(())
        },
    }];
    let v0 = Value::Map(vec![
        (Value::from("seed"), Value::from(4u64)),
        (Value::from("tick"), Value::from(1234u64)),
    ]);
    let mut v1 = migrate::migrate(v0.clone(), 0, 1, &steps).unwrap();
    assert_eq!(
        migrate::field_mut(&mut v1, "ticks").and_then(|x| x.as_u64()),
        Some(1234)
    );
    assert!(migrate::field_mut(&mut v1, "tick").is_none());
    assert_eq!(
        migrate::field_mut(&mut v1, "weather").and_then(|x| x.as_str().map(str::to_owned)),
        Some("clear".to_owned())
    );
    // Identity when already current; a missing step is an error, not a panic.
    assert_eq!(migrate::migrate(v0.clone(), 1, 1, &steps).unwrap(), v0);
    assert!(matches!(
        migrate::migrate(v0, 1, 2, &steps),
        Err(SaveError::Migration(1, _))
    ));
}

#[test]
fn struct_map_missing_field_defaults() {
    let mut s = town(11, TerrainSetup::None);
    s.run_ticks(TICKS_PER_GAME_DAY + 3 * TICKS_PER_GAME_HOUR);
    let bytes = s.save();
    let (_, h, b) = format::split(&bytes).unwrap();
    let mut v: Value = rmp_serde::from_slice(b).unwrap();
    // Drop fields that carry `#[serde(default)]` from every household and citizen.
    let pop = migrate::field_mut(&mut v, "population").unwrap();
    let mut dropped = 0;
    for (table, key) in [
        ("households", "unhappy_days"),
        ("citizens", "commute_sim_s"),
    ] {
        let Value::Map(entries) = migrate::field_mut(pop, table).unwrap() else {
            panic!("{table} map");
        };
        for (_, rec) in entries.iter_mut() {
            let Value::Map(fields) = rec else {
                panic!("record map")
            };
            let before = fields.len();
            fields.retain(|(k, _)| k.as_str() != Some(key));
            dropped += before - fields.len();
        }
    }
    assert!(dropped > 20, "{dropped} fields dropped");
    let file = format::assemble(FORMAT_VERSION, h, &reencode(&v));
    let loaded = Scenario::load(&file).unwrap();
    let pop = loaded.world().resource::<Population>();
    assert!(pop.citizens().all(|(_, c)| c.commute_sim_s().is_none()));
    assert!(pop.households().count() > 10);
}

#[test]
fn save_roundtrip_valley_terrain_continues() {
    let valley = TerrainSetup::Valley {
        samples: 513,
        spacing_m: 4.0,
        h: 20.0,
        depth_m: 9.0,
        width_m: 140.0,
        water_m: Some(2.0),
    };
    let mut a = town(13, valley);
    a.run_ticks(9 * TICKS_PER_GAME_HOUR + 211);
    let bytes = a.save();
    let mut b = Scenario::load(&bytes).unwrap();
    assert_eq!(a.state_hash(), b.state_hash());
    let ta = a
        .world()
        .resource::<cb_sim::terrain::TerrainRes>()
        .terrain();
    let tb = b
        .world()
        .resource::<cb_sim::terrain::TerrainRes>()
        .terrain();
    assert!(ta.base() == tb.base(), "base heights restored exactly");
    assert!(
        ta.height() == tb.height(),
        "composited heights rebuilt exactly"
    );
    a.run_ticks(6 * TICKS_PER_GAME_HOUR);
    b.run_ticks(6 * TICKS_PER_GAME_HOUR);
    assert_eq!(a.state_hash(), b.state_hash());
}
