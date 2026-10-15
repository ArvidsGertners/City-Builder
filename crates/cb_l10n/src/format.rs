//! Locale-aware formatting of numbers, money, percentages, clock times, dates and durations.
//!
//! English: `1,234.5`, `$1,234`, `9.5%`. German: `1.234,5`, `1.234 €`, `9,5 %` (no-break
//! space before `€` and `%`). Both locales use a 24-hour clock (ADR-0021).

use cb_core::money::Money;
use cb_core::time::{Calendar, Season, perceived_minutes};

use crate::{Arg, L10n, Locale, keys};

/// No-break space used between a number and `€` / `%` in German.
pub const NBSP: char = '\u{00A0}';

fn separators(locale: Locale) -> (char, char) {
    match locale {
        Locale::En => (',', '.'),
        Locale::De => ('.', ','),
    }
}

/// Inserts `sep` every three digits from the right of an ASCII digit string.
fn group(digits: &str, sep: char) -> String {
    let n = digits.len();
    let mut out = String::with_capacity(n + n / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (n - i).is_multiple_of(3) {
            out.push(sep);
        }
        out.push(c);
    }
    out
}

/// An integer with thousands grouping (`-1,234` / `-1.234`).
#[must_use]
pub fn int(locale: Locale, value: i64) -> String {
    let (thousands, _) = separators(locale);
    let digits = value.unsigned_abs().to_string();
    let sign = if value < 0 { "-" } else { "" };
    format!("{sign}{}", group(&digits, thousands))
}

/// A decimal number with `decimals` fraction digits (round half to even on the binary value,
/// as Rust's formatter does). Non-finite values render as `–`.
#[must_use]
pub fn decimal(locale: Locale, value: f64, decimals: usize) -> String {
    if !value.is_finite() {
        return "–".to_owned();
    }
    let (thousands, point) = separators(locale);
    let text = format!("{:.*}", decimals, value.abs());
    let (whole, frac) = text.split_once('.').unwrap_or((text.as_str(), ""));
    // "-0.0" is shown as "0.0".
    let negative = value < 0.0 && text.chars().any(|c| c.is_ascii_digit() && c != '0');
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(&group(whole, thousands));
    if !frac.is_empty() {
        out.push(point);
        out.push_str(frac);
    }
    out
}

/// Money in whole currency units, rounded half away from zero (`$1,234` / `1.234 €`).
#[must_use]
pub fn money(locale: Locale, amount: Money) -> String {
    let cents = amount.cents();
    let units = (cents.unsigned_abs() + 50) / 100;
    let sign = if cents < 0 && units > 0 { "-" } else { "" };
    let (thousands, _) = separators(locale);
    let body = group(&units.to_string(), thousands);
    match locale {
        Locale::En => format!("{sign}${body}"),
        Locale::De => format!("{sign}{body}{NBSP}€"),
    }
}

/// A fraction (0.095 = 9.5 %) as a percentage with `decimals` fraction digits.
#[must_use]
pub fn percent(locale: Locale, fraction: f64, decimals: usize) -> String {
    let number = decimal(locale, fraction * 100.0, decimals);
    match locale {
        Locale::En => format!("{number}%"),
        Locale::De => format!("{number}{NBSP}%"),
    }
}

/// A 24-hour clock time `HH:MM`.
#[must_use]
pub fn clock(hour: u32, minute: u32) -> String {
    format!("{hour:02}:{minute:02}")
}

/// The localized season name.
#[must_use]
pub fn season(l10n: &L10n, season: Season) -> String {
    l10n.tr(match season {
        Season::Spring => keys::SEASON_SPRING,
        Season::Summer => keys::SEASON_SUMMER,
        Season::Autumn => keys::SEASON_AUTUMN,
        Season::Winter => keys::SEASON_WINTER,
    })
}

/// The calendar date, e.g. "Year 3, Spring, Day 2".
#[must_use]
pub fn date(l10n: &L10n, cal: &Calendar) -> String {
    let season_name = season(l10n, cal.season);
    l10n.tr_args(
        keys::CALENDAR_DATE,
        &[
            (
                "year",
                Arg::Text(int(l10n.locale(), saturating_i64(cal.year)).into()),
            ),
            ("season", Arg::Text(season_name.into())),
            (
                "day",
                Arg::Text(int(l10n.locale(), saturating_i64(cal.day_of_season)).into()),
            ),
        ],
    )
}

/// A trip duration given in sim seconds, shown as perceived minutes (ADR-0042), at least 1.
#[must_use]
pub fn duration_perceived(l10n: &L10n, sim_seconds: f32) -> String {
    let minutes = perceived_minutes(sim_seconds).round().max(1.0);
    // Perceived durations are small; the cast cannot overflow for realistic values.
    let minutes = if minutes.is_finite() {
        minutes as i64
    } else {
        1
    };
    l10n.tr_args(
        keys::DURATION_MINUTES,
        &[("minutes", Arg::Text(int(l10n.locale(), minutes).into()))],
    )
}

fn saturating_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb_core::time::{TICKS_PER_GAME_DAY, TICKS_PER_GAME_HOUR, TICKS_PER_GAME_MINUTE, Tick};

    #[test]
    fn format_number_money_date_time_en_de() {
        use Locale::{De, En};
        assert_eq!(int(En, 0), "0");
        assert_eq!(int(En, 999), "999");
        assert_eq!(int(En, 1234), "1,234");
        assert_eq!(int(De, 1_234_567), "1.234.567");
        assert_eq!(int(De, -1000), "-1.000");
        assert_eq!(int(En, i64::MIN), "-9,223,372,036,854,775,808");

        assert_eq!(decimal(En, 1234.5, 1), "1,234.5");
        assert_eq!(decimal(De, 1234.5, 1), "1.234,5");
        assert_eq!(decimal(De, -0.04, 1), "0,0");
        assert_eq!(decimal(En, 12.0, 0), "12");
        assert_eq!(decimal(En, f64::NAN, 1), "–");

        assert_eq!(money(En, Money::units(1234)), "$1,234");
        assert_eq!(money(De, Money::units(1234)), "1.234\u{a0}€");
        assert_eq!(money(En, Money(-123_450)), "-$1,235");
        assert_eq!(money(De, Money(-49)), "0\u{a0}€");

        assert_eq!(percent(En, 0.095, 1), "9.5%");
        assert_eq!(percent(De, 0.095, 1), "9,5\u{a0}%");

        assert_eq!(clock(7, 5), "07:05");

        let tick =
            Tick(TICKS_PER_GAME_DAY * 9 + TICKS_PER_GAME_HOUR * 7 + TICKS_PER_GAME_MINUTE * 5);
        let cal = Calendar::from_tick(tick, 4);
        let en = L10n::new(En).unwrap();
        let de = L10n::new(De).unwrap();
        assert_eq!(date(&en, &cal), "Year 1, Autumn, Day 2");
        assert_eq!(date(&de, &cal), "Jahr 1, Herbst, Tag 2");
        assert_eq!(clock(cal.hour, cal.minute), "07:05");

        // 150 sim s = 15 perceived minutes.
        assert_eq!(duration_perceived(&en, 150.0), "15 min");
        assert_eq!(duration_perceived(&de, 150.0), "15 Min.");
        assert_eq!(duration_perceived(&de, 0.0), "1 Min.");
    }
}
