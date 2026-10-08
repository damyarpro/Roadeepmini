// Solar Hijri (Jalali) ⇄ Gregorian, and the civil-date arithmetic the planner
// needs, without a crate. The Jalali side is the arithmetic of jalaali-js
// (Borkowski's break years), the same rules src/core/jalali.ts follows, so the
// chat tools and the island always print the same date for a day.
//
// Storage never uses Jalali: day keys are Gregorian "YYYY-MM-DD" local dates.

/// Jalali years where the 33-year leap cycle shifts (jalaali-js `breaks`).
const BREAKS: [i64; 20] = [
    -61, 9, 38, 199, 426, 686, 756, 818, 1111, 1181, 1210, 1635, 2060, 2097, 2192, 2262, 2324, 2394, 2456, 3178,
];

/// Persian weekday names, Saturday first (the Iranian week).
pub const WEEKDAYS_FA: [&str; 7] = ["شنبه", "یکشنبه", "دوشنبه", "سه‌شنبه", "چهارشنبه", "پنجشنبه", "جمعه"];

/// (leap index, Gregorian year of Farvardin 1, March day of Farvardin 1);
/// None outside the years the break table covers.
fn jal_cal(jy: i64) -> Option<(i64, i64, i64)> {
    let gy = jy + 621;
    if jy < BREAKS[0] || jy >= BREAKS[BREAKS.len() - 1] {
        return None;
    }
    let mut leap_j = -14;
    let mut jp = BREAKS[0];
    let mut jump = 0;
    for &jm in &BREAKS[1..] {
        jump = jm - jp;
        if jy < jm {
            break;
        }
        leap_j += jump / 33 * 8 + (jump % 33) / 4;
        jp = jm;
    }
    let mut n = jy - jp;
    leap_j += n / 33 * 8 + ((n % 33) + 3) / 4;
    if jump % 33 == 4 && jump - n == 4 {
        leap_j += 1;
    }
    let leap_g = gy / 4 - ((gy / 100 + 1) * 3) / 4 - 150;
    let march = 20 + leap_j - leap_g;
    if jump - n < 6 {
        n = n - jump + ((jump + 4) / 33) * 33;
    }
    let mut leap = (((n + 1) % 33) - 1) % 4;
    if leap == -1 {
        leap = 4;
    }
    Some((leap, gy, march))
}

/// Gregorian date → Julian day number.
fn g2d(gy: i64, gm: i64, gd: i64) -> i64 {
    let d = ((gy + (gm - 8) / 6 + 100_100) * 1461) / 4 + (153 * ((gm + 9) % 12) + 2) / 5 + gd - 34_840_408;
    d - ((gy + 100_100 + (gm - 8) / 6) / 100 * 3) / 4 + 752
}

/// Julian day number → Gregorian date.
fn d2g(jdn: i64) -> (i64, i64, i64) {
    let mut j = 4 * jdn + 139_361_631;
    j += (((4 * jdn + 183_187_720) / 146_097) * 3) / 4 * 4 - 3908;
    let i = ((j % 1461) / 4) * 5 + 308;
    let gd = (i % 153) / 5 + 1;
    let gm = ((i / 153) % 12) + 1;
    let gy = j / 1461 - 100_100 + (8 - gm) / 6;
    (gy, gm, gd)
}

/// Julian day number → Jalali date.
fn d2j(jdn: i64) -> Option<(i64, i64, i64)> {
    let (gy, _, _) = d2g(jdn);
    let mut jy = gy - 621;
    let (leap, _, march) = jal_cal(jy)?;
    let mut k = jdn - g2d(gy, 3, march);
    if k >= 0 {
        if k <= 185 {
            return Some((jy, 1 + k / 31, k % 31 + 1));
        }
        k -= 186;
    } else {
        jy -= 1;
        k += 179;
        if leap == 1 {
            k += 1;
        }
    }
    Some((jy, 7 + k / 30, k % 30 + 1))
}

/// Jalali date → Julian day number.
fn j2d(jy: i64, jm: i64, jd: i64) -> Option<i64> {
    let (_, gy, march) = jal_cal(jy)?;
    Some(g2d(gy, 3, march) + (jm - 1) * 31 - (jm / 7) * (jm - 7) + jd - 1)
}

/// Gregorian → Jalali (year, month, day).
pub fn to_jalali(gy: i64, gm: u32, gd: u32) -> Option<(i64, u32, u32)> {
    let (jy, jm, jd) = d2j(g2d(gy, i64::from(gm), i64::from(gd)))?;
    Some((jy, jm as u32, jd as u32))
}

/// Jalali → Gregorian (year, month, day); None for a day that doesn't exist
/// (Esfand 30 of a common year, month 13…).
pub fn to_gregorian(jy: i64, jm: u32, jd: u32) -> Option<(i64, u32, u32)> {
    if !(1..=12).contains(&jm) || jd == 0 || jd > 31 {
        return None;
    }
    let (gy, gm, gd) = d2g(j2d(jy, i64::from(jm), i64::from(jd))?);
    // Round trip: a day past the month's end lands in the next month.
    (to_jalali(gy, gm as u32, gd as u32)? == (jy, jm, jd)).then_some((gy, gm as u32, gd as u32))
}

// ── Civil (proleptic Gregorian) days ──────────────────────────────────────────

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `days` after 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Day of the week with Saturday = 0 … Friday = 6 (1970-01-01 was a Thursday).
pub fn weekday(days: i64) -> u32 {
    (days + 5).rem_euclid(7) as u32
}

/// "YYYY-MM-DD" for `days` since 1970-01-01.
pub fn day_key_of(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 of a strict "YYYY-MM-DD" key that names a real date.
pub fn days_of_key(key: &str) -> Option<i64> {
    let b = key.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let num = |s: &str| -> Option<u32> { s.bytes().all(|c| c.is_ascii_digit()).then(|| s.parse().ok()).flatten() };
    let (y, m, d) = (num(&key[0..4])?, num(&key[5..7])?, num(&key[8..10])?);
    if !(1900..=9999).contains(&y) || !(1..=12).contains(&m) || d == 0 {
        return None;
    }
    let days = days_from_civil(i64::from(y), m, d);
    (civil_from_days(days) == (i64::from(y), m, d)).then_some(days)
}

/// The Persian name of the weekday of `days` since 1970-01-01.
pub fn weekday_fa(days: i64) -> &'static str {
    WEEKDAYS_FA[weekday(days) as usize]
}

/// Days since 1970-01-01 of a Jalali "1405/07/15" (or 1405-07-15); None when
/// it isn't one, or names a day that doesn't exist.
pub fn days_of_jalali(text: &str) -> Option<i64> {
    let parts: Vec<&str> = text.split(['/', '-']).collect();
    let [y, m, d] = parts.as_slice() else { return None };
    let num = |s: &str, max_len: usize| (!s.is_empty() && s.len() <= max_len && s.bytes().all(|c| c.is_ascii_digit())).then(|| s.parse::<u32>().ok()).flatten();
    let (y, m, d) = (num(y, 4)?, num(m, 2)?, num(d, 2)?);
    if !(1300..=1500).contains(&y) {
        return None;
    }
    let (gy, gm, gd) = to_gregorian(i64::from(y), m, d)?;
    Some(days_from_civil(gy, gm, gd))
}

/// "1405/01/15" for a Gregorian day key; None for an invalid key.
pub fn jalali_of_key(key: &str) -> Option<String> {
    let (y, m, d) = civil_from_days(days_of_key(key)?);
    let (jy, jm, jd) = to_jalali(y, m, d)?;
    Some(format!("{jy:04}/{jm:02}/{jd:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_pairs_convert_both_ways() {
        let pairs = [
            ((2026, 3, 21), (1405, 1, 1)),
            ((2025, 3, 20), (1403, 12, 30)),
            ((2024, 2, 29), (1402, 12, 10)),
            ((2027, 3, 21), (1406, 1, 1)),
            ((2025, 3, 21), (1404, 1, 1)),
            ((1979, 2, 11), (1357, 11, 22)),
            ((2026, 10, 3), (1405, 7, 11)),
        ];
        for ((gy, gm, gd), (jy, jm, jd)) in pairs {
            assert_eq!(to_jalali(gy, gm, gd), Some((jy, jm, jd)), "{gy}-{gm}-{gd}");
            assert_eq!(to_gregorian(jy, jm, jd), Some((gy, gm, gd)), "{jy}/{jm}/{jd}");
        }
    }

    #[test]
    fn impossible_jalali_days_are_refused() {
        assert!(to_gregorian(1403, 12, 30).is_some(), "1403 is a leap year");
        assert_eq!(to_gregorian(1404, 12, 30), None, "1404 is not");
        assert_eq!(to_gregorian(1405, 7, 31), None, "Mehr has 30 days");
        assert_eq!(to_gregorian(1405, 13, 1), None);
        assert_eq!(to_gregorian(1405, 1, 0), None);
    }

    #[test]
    fn every_day_of_a_decade_round_trips() {
        let start = days_from_civil(2020, 1, 1);
        for days in start..start + 3653 {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
            let (jy, jm, jd) = to_jalali(y, m, d).unwrap();
            assert_eq!(to_gregorian(jy, jm, jd), Some((y, m, d)));
        }
    }

    #[test]
    fn day_keys_and_weekdays() {
        assert_eq!(days_of_key("1970-01-01"), Some(0));
        assert_eq!(day_key_of(days_of_key("2026-03-21").unwrap()), "2026-03-21");
        for bad in ["2026-02-30", "2026-13-01", "2026-1-01", "26-01-01", "2026/01/01", "２026-01-01", "", "2026-01-01 "] {
            assert_eq!(days_of_key(bad), None, "{bad}");
        }
        // 2026-03-21 (Nowruz 1405) is a Saturday.
        assert_eq!(weekday(days_of_key("2026-03-21").unwrap()), 0);
        assert_eq!(WEEKDAYS_FA[weekday(days_of_key("2026-10-03").unwrap()) as usize], "شنبه");
        assert_eq!(weekday(days_of_key("2026-10-09").unwrap()), 6, "a Friday");
        assert_eq!(jalali_of_key("2026-03-21").as_deref(), Some("1405/01/01"));
        assert_eq!(jalali_of_key("nope"), None);
        assert_eq!(weekday_fa(days_of_key("2026-10-04").unwrap()), "یکشنبه");
        assert_eq!(days_of_jalali("1405/07/11"), days_of_key("2026-10-03"));
        assert_eq!(days_of_jalali("1405-1-1"), days_of_key("2026-03-21"));
        for bad in ["1404/12/30", "2026/10/03", "1405/07", "۱۴۰۵/۰۷/۱۱", "1405/07/11/1", "a/b/c"] {
            assert_eq!(days_of_jalali(bad), None, "{bad}");
        }
    }
}
