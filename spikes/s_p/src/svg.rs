//! SVG contact sheets for visual review.

use std::fmt::Write as _;

use glam::DVec2;

use crate::fit::Fit;
use crate::frontage::Frontage;
use crate::geom::{self, Poly};
use crate::subdivide::Output;

pub struct Cell<'a> {
    pub title: String,
    pub f: &'a Frontage,
    pub plot: &'a Poly,
    pub out: &'a Output,
    pub fits: &'a [Fit],
    /// Stable IDs per lot (colour key).
    pub ids: Vec<usize>,
}

const CW: f64 = 320.0;
const CH: f64 = 320.0;

fn path(p: &[DVec2], tf: &impl Fn(DVec2) -> DVec2, close: bool) -> String {
    let mut s = String::new();
    for (i, v) in p.iter().enumerate() {
        let q = tf(*v);
        let _ = write!(
            s,
            "{}{:.1},{:.1} ",
            if i == 0 { "M" } else { "L" },
            q.x,
            q.y
        );
    }
    if close {
        s.push('Z');
    }
    s
}

pub fn sheet(cells: &[Cell], cols: usize, file: &str) -> std::io::Result<()> {
    let rows = cells.len().div_ceil(cols);
    let (w, h) = (CW * cols as f64, CH * rows as f64);
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="sans-serif">"#
    );
    let _ = writeln!(s, r##"<rect width="{w}" height="{h}" fill="#f4f1ea"/>"##);
    for (ci, c) in cells.iter().enumerate() {
        let ox = (ci % cols) as f64 * CW;
        let oy = (ci / cols) as f64 * CH;
        let mut all: Vec<DVec2> = c.plot.clone();
        all.extend(&c.f.pts);
        let (lo, hi) = geom::bbox(&all);
        let ext = (hi - lo).max_element().max(1.0);
        let k = (CW - 30.0) / (ext + 16.0);
        let mid = (lo + hi) * 0.5;
        let tf = move |p: DVec2| {
            DVec2::new(
                ox + CW * 0.5 + (p.x - mid.x) * k,
                oy + CH * 0.5 + 8.0 - (p.y - mid.y) * k,
            )
        };
        let _ = writeln!(
            s,
            r##"<rect x="{ox}" y="{oy}" width="{CW}" height="{CH}" fill="none" stroke="#ccc"/>"##
        );
        // Road: thick band outside the frontage.
        let road: Vec<DVec2> =
            c.f.pts
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let j = i.min(c.f.pts.len() - 2);
                    *p - c.f.seg_normal(j) * 4.0
                })
                .collect();
        let _ = writeln!(
            s,
            r##"<path d="{}" fill="none" stroke="#8a8a8a" stroke-width="{:.1}" stroke-linejoin="round"/>"##,
            path(&road, &tf, false),
            7.0 * k
        );
        for (li, lot) in c.out.lots.iter().enumerate() {
            let id = c.ids.get(li).copied().unwrap_or(li);
            let hue = (id as f64 * 137.508) % 360.0;
            let fill = if lot.corner {
                "hsl(30,80%,70%)".to_owned()
            } else {
                format!("hsl({hue:.0},55%,78%)")
            };
            let _ = writeln!(
                s,
                r##"<path d="{}" fill="{fill}" stroke="#333" stroke-width="0.8"/>"##,
                path(&lot.poly, &tf, true)
            );
        }
        for fit in c.fits {
            if let Some(ins) = &fit.inset {
                let _ = writeln!(
                    s,
                    r##"<path d="{}" fill="none" stroke="#666" stroke-width="0.5" stroke-dasharray="2,2"/>"##,
                    path(ins, &tf, true)
                );
            }
            if let Some(fp) = &fit.footprint {
                let _ = writeln!(
                    s,
                    r##"<path d="{}" fill="#8b5a3c" fill-opacity="0.85" stroke="#4a2c1a" stroke-width="0.6"/>"##,
                    path(fp, &tf, true)
                );
            }
            if fit.green {
                // nothing: lot stays green
            }
        }
        for sm in &c.out.seams {
            let _ = writeln!(
                s,
                r##"<path d="{}" fill="none" stroke="#d22" stroke-width="1" stroke-dasharray="4,2"/>"##,
                path(sm, &tf, false)
            );
        }
        // Lot labels: rectangularity / fan.
        for lot in &c.out.lots {
            let cen = lot.poly.iter().copied().sum::<DVec2>() / lot.poly.len() as f64;
            let q = tf(cen);
            let col = if lot.rect < 0.55 || (!lot.corner && lot.fan_deg() > 25.0) {
                "#c00"
            } else {
                "#222"
            };
            let _ = writeln!(
                s,
                r#"<text x="{:.1}" y="{:.1}" font-size="7" text-anchor="middle" fill="{col}">{:.2}/{:.0}°</text>"#,
                q.x,
                q.y,
                lot.rect,
                lot.fan_deg()
            );
        }
        let _ = writeln!(
            s,
            r##"<text x="{}" y="{}" font-size="11" fill="#111">{}</text>"##,
            ox + 6.0,
            oy + 14.0,
            c.title
        );
    }
    s.push_str("</svg>\n");
    std::fs::write(file, s)
}
