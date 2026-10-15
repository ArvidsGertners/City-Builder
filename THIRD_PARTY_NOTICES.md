# Third-Party Notices

## Fonts

| Component | Version | Files | Licence |
|---|---|---|---|
| Inter (The Inter Project Authors, https://github.com/rsms/inter) | 4.001 | `crates/cb_game/assets/fonts/Inter-Regular.ttf`, `Inter-SemiBold.ttf` | SIL Open Font License 1.1 — full text in `crates/cb_game/assets/fonts/OFL.txt` |
| egui default fonts (Ubuntu-Light, Hack, Noto Emoji, emoji-icon-font), bundled by the `egui` crate | egui 0.36 | linked into the binary | Ubuntu Font Licence 1.0, MIT/Bitstream Vera (Hack), SIL OFL 1.1 / Apache-2.0 (Noto Emoji), MIT (emoji-icon-font) |

## Rust crates

The game links open-source crates (Bevy, bevy_egui, egui, wgpu, serde, tracing, fluent, i_overlay,
lz4_flex, rmp-serde, libm, and their dependencies) under MIT, Apache-2.0, Zlib, BSD or similar
permissive licences. A generated, complete list of crate licences is bundled with release
artifacts (planned for M16; until then `cargo tree` in this repository lists them).
