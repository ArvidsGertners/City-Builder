//! Helpers for seeded randomized tests (no proptest dependency, ADR-0023).

use crate::rng::Pcg32;

/// `n` independent generators for randomized test cases; case `i` is reproducible from
/// `(seed, i)` so a failing case can be pinned.
pub fn cases(seed: u64, n: usize) -> impl Iterator<Item = (usize, Pcg32)> {
    (0..n).map(move |i| (i, Pcg32::new(seed, i as u64)))
}

/// Greedy shrinker: repeatedly replaces `input` with the first candidate from `shrink` that
/// still `fails`, until no candidate fails. Returns the minimal failing input found.
pub fn minimize<T: Clone>(
    input: T,
    shrink: impl Fn(&T) -> Vec<T>,
    fails: impl Fn(&T) -> bool,
) -> T {
    let mut current = input;
    let mut guard = 0;
    'outer: while guard < 10_000 {
        guard += 1;
        for candidate in shrink(&current) {
            if fails(&candidate) {
                current = candidate;
                continue 'outer;
            }
        }
        break;
    }
    current
}

/// Standard shrink candidates for a `usize`: 0, half, minus one.
#[must_use]
pub fn shrink_usize(n: &usize) -> Vec<usize> {
    let mut out = Vec::new();
    if *n > 0 {
        out.push(0);
        out.push(n / 2);
        out.push(n - 1);
    }
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimize_finds_threshold() {
        // Fails for n >= 37: minimal failing input is 37.
        assert_eq!(minimize(1000usize, shrink_usize, |n| *n >= 37), 37);
    }

    #[test]
    fn cases_are_reproducible() {
        let a: Vec<u32> = cases(5, 3).map(|(_, mut r)| r.next_u32()).collect();
        let b: Vec<u32> = cases(5, 3).map(|(_, mut r)| r.next_u32()).collect();
        assert_eq!(a, b);
    }
}
