//! The UI font: Inter 4.001 (SIL OFL 1.1), Regular and SemiBold, embedded (ADR-0059).

use std::sync::Arc;

use bevy_egui::egui;

/// Inter Regular.
pub const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
/// Inter SemiBold.
pub const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");

/// Name of the SemiBold family for headings.
pub const SEMIBOLD_FAMILY: &str = "Inter SemiBold";

/// Characters the UI must render beyond printable ASCII: German, typography, currency and
/// the macOS shortcut glyphs (ADR-0059).
pub const REQUIRED_GLYPHS: &str = "ÄÖÜäöüßẞ„“‚‘»«–—…€§°²³×÷±½¼¾µ·•→←↑↓↔⌘⌥⇧⌃⌫⏎⎋⇥✓✗\u{00A0}\u{2212}";

/// Replaces egui's default proportional font with Inter (egui's fonts stay as fallback).
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".to_owned(),
        Arc::new(egui::FontData::from_static(INTER_REGULAR)),
    );
    fonts.font_data.insert(
        SEMIBOLD_FAMILY.to_owned(),
        Arc::new(egui::FontData::from_static(INTER_SEMIBOLD)),
    );
    if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        list.insert(0, "Inter".to_owned());
    }
    fonts.families.insert(
        egui::FontFamily::Name(SEMIBOLD_FAMILY.into()),
        vec![SEMIBOLD_FAMILY.to_owned(), "Inter".to_owned()],
    );
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be16(d: &[u8], o: usize) -> u16 {
        u16::from_be_bytes([d[o], d[o + 1]])
    }
    fn be32(d: &[u8], o: usize) -> u32 {
        u32::from_be_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]])
    }

    /// Whether the font's Unicode cmap (format 4 or 12) maps `c` to a glyph.
    fn has_glyph(font: &[u8], c: char) -> bool {
        let tables = be16(font, 4) as usize;
        let cmap = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|&r| &font[r..r + 4] == b"cmap")
            .map(|r| be32(font, r + 8) as usize)
            .expect("cmap table");
        let n = be16(font, cmap + 2) as usize;
        let cp = c as u32;
        for i in 0..n {
            let rec = cmap + 4 + 8 * i;
            let (platform, encoding) = (be16(font, rec), be16(font, rec + 2));
            if !(platform == 0 || (platform == 3 && (encoding == 1 || encoding == 10))) {
                continue;
            }
            let sub = cmap + be32(font, rec + 4) as usize;
            match be16(font, sub) {
                4 if cp <= 0xFFFF => {
                    let segs = be16(font, sub + 6) as usize / 2;
                    let ends = sub + 14;
                    let starts = ends + 2 * segs + 2;
                    let deltas = starts + 2 * segs;
                    let offsets = deltas + 2 * segs;
                    for s in 0..segs {
                        let (end, start) = (
                            u32::from(be16(font, ends + 2 * s)),
                            u32::from(be16(font, starts + 2 * s)),
                        );
                        if cp < start || cp > end {
                            continue;
                        }
                        let ro = be16(font, offsets + 2 * s) as usize;
                        let glyph = if ro == 0 {
                            (cp as u16).wrapping_add(be16(font, deltas + 2 * s))
                        } else {
                            let addr = offsets + 2 * s + ro + 2 * (cp - start) as usize;
                            let g = be16(font, addr);
                            if g == 0 {
                                0
                            } else {
                                g.wrapping_add(be16(font, deltas + 2 * s))
                            }
                        };
                        if glyph != 0 {
                            return true;
                        }
                    }
                }
                12 => {
                    let groups = be32(font, sub + 12) as usize;
                    for g in 0..groups {
                        let o = sub + 16 + 12 * g;
                        if (be32(font, o)..=be32(font, o + 4)).contains(&cp)
                            && be32(font, o + 8) != 0
                        {
                            return true;
                        }
                    }
                }
                _ => {}
            }
        }
        false
    }

    #[test]
    fn font_covers_required_glyphs() {
        let ascii = (0x20u8..0x7F).map(char::from);
        let locales = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../cb_l10n/locales");
        let mut ftl = String::new();
        for lang in ["en", "de"] {
            for entry in std::fs::read_dir(locales.join(lang)).unwrap() {
                ftl.push_str(&std::fs::read_to_string(entry.unwrap().path()).unwrap());
            }
        }
        for font in [INTER_REGULAR, INTER_SEMIBOLD] {
            for c in ascii
                .clone()
                .chain(REQUIRED_GLYPHS.chars())
                .chain(ftl.chars().filter(|c| !c.is_control()))
            {
                assert!(
                    has_glyph(font, c),
                    "missing glyph {c:?} (U+{:04X})",
                    c as u32
                );
            }
            assert!(
                !has_glyph(font, '\u{4E00}'),
                "parser sanity: CJK is not in Inter"
            );
        }
    }
}
