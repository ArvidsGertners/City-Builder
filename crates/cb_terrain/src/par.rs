//! Deterministic row-parallel helpers: work is split into `PARTITIONS` fixed row bands and each
//! band is computed independently, so results never depend on the thread count (ADR-0043).

/// Fixed number of row bands (same value as `SIM_PARTITIONS`).
pub(crate) const PARTITIONS: usize = 64;

/// Calls `f(row, out_row)` for every row of an `n × n` grid, rows split over threads in fixed
/// bands; `f` must only depend on its inputs.
pub(crate) fn for_rows<T: Send>(data: &mut [T], n: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let threads = std::thread::available_parallelism()
        .map_or(1, std::num::NonZeroUsize::get)
        .min(PARTITIONS);
    let band = n.div_ceil(PARTITIONS).max(1);
    let mut bands: Vec<(usize, &mut [T])> = data
        .chunks_mut(band * n)
        .enumerate()
        .map(|(b, c)| (b * band, c))
        .collect();
    if threads <= 1 {
        for (row0, chunk) in &mut bands {
            for (k, row) in chunk.chunks_mut(n).enumerate() {
                f(*row0 + k, row);
            }
        }
        return;
    }
    let per = bands.len().div_ceil(threads);
    std::thread::scope(|s| {
        for group in bands.chunks_mut(per) {
            let f = &f;
            s.spawn(move || {
                for (row0, chunk) in group.iter_mut() {
                    for (k, row) in chunk.chunks_mut(n).enumerate() {
                        f(*row0 + k, row);
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_cover_grid_in_order() {
        let n = 130;
        let mut d = vec![0usize; n * n];
        for_rows(&mut d, n, |j, row| {
            for (i, v) in row.iter_mut().enumerate() {
                *v = j * n + i;
            }
        });
        assert!(d.iter().enumerate().all(|(k, v)| k == *v));
    }
}
