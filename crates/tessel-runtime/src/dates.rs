//! Dates and times of the standard library. A moment is a `Float`: seconds
//! since 1 January 1970 (UTC). What the calendar and the clock say then
//! depends on the time zone: the computer's (`""`), a named one
//! (`"Europe/Paris"`, from the tz database built into the program), `"UTC"`,
//! or a fixed distance from UTC (`"+05:30"`).
//!
//! A time zone that doesn't exist is treated like a date that doesn't:
//! `""`, `nil`, `NaN` or zeros come back, not an error.

use chrono::{DateTime, Datelike, Duration, FixedOffset, Local, Months, NaiveDate, NaiveDateTime, Offset, SecondsFormat, TimeZone, Timelike, Utc};

use crate::{str_new, List, Str};

enum Zone {
    Local,
    Named(chrono_tz::Tz),
    Fixed(FixedOffset),
}

impl Zone {
    fn parse(name: &str) -> Option<Zone> {
        let name = name.trim();
        if name.is_empty() {
            return Some(Zone::Local);
        }
        if name.eq_ignore_ascii_case("utc") || name == "Z" {
            return FixedOffset::east_opt(0).map(Zone::Fixed);
        }
        // "+05:30", "-0800", "+2"
        if let Some(distance) = name.strip_prefix(['+', '-']) {
            let digits: String = distance.chars().filter(|c| *c != ':').collect();
            if digits.is_empty() || digits.len() > 4 || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let (hours, minutes) = if digits.len() <= 2 { (digits.as_str(), "0") } else { digits.split_at(digits.len() - 2) };
            let seconds = hours.parse::<i32>().ok()? * 3600 + minutes.parse::<i32>().ok()? * 60;
            return FixedOffset::east_opt(if name.starts_with('-') { -seconds } else { seconds }).map(Zone::Fixed);
        }
        name.parse().ok().map(Zone::Named)
    }

    /// The moment `time`, as the date and time it is in this zone.
    fn at(&self, time: f64) -> Option<DateTime<FixedOffset>> {
        if !time.is_finite() {
            return None;
        }
        let utc = DateTime::<Utc>::from_timestamp(time.floor() as i64, ((time - time.floor()) * 1e9) as u32)?;
        let offset = match self {
            Zone::Local => Local.offset_from_utc_datetime(&utc.naive_utc()).fix(),
            Zone::Named(tz) => tz.offset_from_utc_datetime(&utc.naive_utc()).fix(),
            Zone::Fixed(offset) => *offset,
        };
        Some(utc.with_timezone(&offset))
    }

    /// The moment the clocks of this zone show `local`: the first one, when
    /// they show it twice (they were set back); when they skip it (they were
    /// set forward), the moment an hour later.
    fn moment(&self, local: &NaiveDateTime) -> Option<f64> {
        fn first<T: TimeZone>(zone: &T, local: &NaiveDateTime) -> Option<i64> {
            let later = *local + Duration::hours(1);
            zone.from_local_datetime(local).earliest().or_else(|| zone.from_local_datetime(&later).earliest()).map(|d| d.timestamp())
        }
        let seconds = match self {
            Zone::Local => first(&Local, local),
            Zone::Named(tz) => first(tz, local),
            Zone::Fixed(offset) => first(offset, local),
        }?;
        Some(seconds as f64)
    }
}

unsafe fn zone(name: *const Str) -> Option<Zone> {
    Zone::parse(unsafe { crate::str_text(name) })
}

/// Tessel date patterns: yyyy MM dd HH mm ss, plus MMM (Jan), MMMM
/// (January), EEE (Mon), EEEE (Monday), a (AM/PM), hh (12-hour), SSS
/// (milliseconds), Z (+0200) and ZZZZZ (+02:00).
fn to_chrono(format: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // 'text' is literal, and '' is a quote (as in Unicode date patterns).
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
                continue;
            }
            i += 1;
            while i < chars.len() {
                match (chars[i], chars.get(i + 1)) {
                    ('\'', Some('\'')) => {
                        out.push('\'');
                        i += 2;
                    }
                    ('\'', _) => {
                        i += 1;
                        break;
                    }
                    ('%', _) => {
                        out.push_str("%%");
                        i += 1;
                    }
                    (ch, _) => {
                        out.push(ch);
                        i += 1;
                    }
                }
            }
            continue;
        }
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        let spec = match (c, n) {
            ('y', 4) => "%Y",
            ('y', 2) => "%y",
            ('M', 4) => "%B",
            ('M', 3) => "%b",
            ('M', 2) => "%m",
            ('M', 1) => "%-m",
            ('d', 2) => "%d",
            ('d', 1) => "%-d",
            ('H', 2) => "%H",
            ('H', 1) => "%-H",
            ('h', 2) => "%I",
            ('h', 1) => "%-I",
            ('m', 2) => "%M",
            ('m', 1) => "%-M",
            ('s', 2) => "%S",
            ('s', 1) => "%-S",
            ('S', 3) => "%3f",
            ('E', 4) => "%A",
            ('E', 3) => "%a",
            ('a', 1) => "%p",
            ('Z', 1) => "%z",
            ('Z', 5) => "%:z",
            ('%', _) => "%%",
            _ => "",
        };
        if spec.is_empty() {
            for _ in 0..n {
                out.push(c);
            }
        } else {
            out.push_str(spec);
        }
        i += n;
    }
    out
}

/// The format name for the internet's date and time format (RFC 3339, a
/// profile of ISO 8601): `2026-03-07T09:05:30+01:00`.
const ISO: &str = "iso";

/// `formatDate(time, format:timeZone:)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_format_date(time: f64, format: *const Str, zone_name: *const Str) -> *mut Str {
    let Some(dt) = unsafe { zone(zone_name) }.and_then(|z| z.at(time)) else { return str_new("") };
    let format = unsafe { crate::str_text(format) };
    if format == ISO {
        return str_new(&dt.to_rfc3339_opts(SecondsFormat::Secs, true));
    }
    str_new(&dt.format(&to_chrono(format)).to_string())
}

/// `parseDate(text, format:timeZone:)`: nil if it doesn't match. The time
/// zone is for text that doesn't say how far from UTC it is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_parse_date(s: *const Str, format: *const Str, zone_name: *const Str, out: *mut f64) -> u8 {
    let (text, format) = unsafe { (crate::str_text(s), crate::str_text(format)) };
    let exact = |dt: DateTime<FixedOffset>| dt.timestamp() as f64 + dt.timestamp_subsec_nanos() as f64 / 1e9;
    let found = (|| {
        let zone = unsafe { zone(zone_name) }?;
        if format == ISO {
            if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
                return Some(exact(dt));
            }
            // Without a distance from UTC, and maybe without a time.
            let local = NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .or_else(|| NaiveDate::parse_from_str(text, "%Y-%m-%d").ok().and_then(|d| d.and_hms_opt(0, 0, 0)))?;
            return zone.moment(&local);
        }
        let pattern = to_chrono(format);
        if pattern.contains("%z") || pattern.contains("%:z") {
            return DateTime::parse_from_str(text, &pattern).ok().map(exact);
        }
        let local = NaiveDateTime::parse_from_str(text, &pattern)
            .ok()
            .or_else(|| NaiveDate::parse_from_str(text, &pattern).ok().and_then(|d| d.and_hms_opt(0, 0, 0)))?;
        zone.moment(&local)
    })();
    match found {
        Some(v) => {
            unsafe { *out = v };
            1
        }
        None => 0,
    }
}

/// `date(year:month:day:hour:minute:second:timeZone:)`: NaN if it isn't a real date.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_make_date(y: i64, mo: i64, d: i64, h: i64, mi: i64, s: i64, zone_name: *const Str) -> f64 {
    let local = NaiveDate::from_ymd_opt(y as i32, mo as u32, d as u32).and_then(|date| date.and_hms_opt(h as u32, mi as u32, s as u32));
    let moment = local.and_then(|local| unsafe { zone(zone_name) }?.moment(&local));
    moment.unwrap_or(f64::NAN)
}

/// For `dateParts`: year, month, day, hour, minute, second, weekday (1 for
/// Monday to 7), day of the year. Zeros for a time or zone that doesn't exist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_date_parts(time: f64, zone_name: *const Str) -> *mut List {
    let parts = match unsafe { zone(zone_name) }.and_then(|z| z.at(time)) {
        Some(dt) => [
            dt.year() as i64,
            dt.month() as i64,
            dt.day() as i64,
            dt.hour() as i64,
            dt.minute() as i64,
            dt.second() as i64,
            dt.weekday().number_from_monday() as i64,
            dt.ordinal() as i64,
        ],
        None => [0; 8],
    };
    let mut list = unsafe { crate::tsl_list_new(&crate::codec::INT_TYPE, parts.len() as i64) };
    for v in parts {
        unsafe { crate::tsl_list_push(&mut list, &v as *const i64 as *const u8) };
    }
    list
}

/// `addToDate(time, years:months:days:hours:minutes:seconds:timeZone:)`:
/// years, months and days move the date on the zone's calendar (to the last
/// day of a shorter month, and to the same time of day across changes of
/// the clocks); hours, minutes and seconds are added to the result.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_add_to_date(
    time: f64,
    years: i64,
    months: i64,
    days: i64,
    hours: i64,
    minutes: i64,
    seconds: i64,
    zone_name: *const Str,
) -> f64 {
    let moved = (|| {
        let zone = unsafe { zone(zone_name) }?;
        let local = zone.at(time)?.naive_local();
        let months = years.checked_mul(12)?.checked_add(months)?;
        let by = Months::new(u32::try_from(months.unsigned_abs()).ok()?);
        let local = if months >= 0 { local.checked_add_months(by) } else { local.checked_sub_months(by) }?;
        let local = local.checked_add_signed(Duration::try_days(days)?)?;
        // (Keep the fraction of a second.)
        Some(zone.moment(&local)? + (time - time.floor()))
    })();
    moved.map_or(f64::NAN, |t| t + (hours * 3600 + minutes * 60 + seconds) as f64)
}

/// `daysBetween(from, to, timeZone:)`: how many times the date changes from
/// one moment to the other, on the zone's calendar (negative if `to` is
/// earlier). 0 for a time or zone that doesn't exist.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_days_between(from: f64, to: f64, zone_name: *const Str) -> i64 {
    let days = (|| {
        let zone = unsafe { zone(zone_name) }?;
        Some((zone.at(to)?.date_naive() - zone.at(from)?.date_naive()).num_days())
    })();
    days.unwrap_or(0)
}

/// `timeZoneOffset(time, timeZone:)`: how many seconds the zone's clocks are
/// ahead of UTC at that moment (negative: behind).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_time_zone_offset(time: f64, zone_name: *const Str) -> i64 {
    unsafe { zone(zone_name) }.and_then(|z| z.at(time)).map_or(0, |dt| dt.offset().local_minus_utc() as i64)
}

/// `localTimeZone()`: the name of the computer's time zone, like
/// "Europe/Paris" ("" if it can't be found out).
#[unsafe(no_mangle)]
pub extern "C" fn tsl_local_time_zone() -> *mut Str {
    str_new(&iana_time_zone::get_timezone().unwrap_or_default())
}

/// `timeZones()`: the names of all time zones, in alphabetical order.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn tsl_time_zones() -> *mut List {
    unsafe { crate::str_list(chrono_tz::TZ_VARIANTS.iter().map(|tz| tz.name())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> *mut Str {
        str_new(text)
    }

    unsafe fn text(s: *const Str) -> String {
        unsafe { crate::str_text(s) }.to_string()
    }

    #[test]
    fn dates_in_the_local_zone() {
        unsafe {
            let t = tsl_make_date(2026, 9, 27, 14, 5, 9, s(""));
            assert_eq!(text(tsl_format_date(t, s("yyyy-MM-dd HH:mm:ss"), s(""))), "2026-09-27 14:05:09");
            assert_eq!(text(tsl_format_date(t, s("EEE, d MMM yyyy h:mm a"), s(""))), "Sun, 27 Sep 2026 2:05 PM");
            let mut back = 0.0;
            assert_eq!(tsl_parse_date(s("2026-09-27 14:05:09"), s("yyyy-MM-dd HH:mm:ss"), s(""), &mut back), 1);
            assert_eq!(back, t);
            assert_eq!(tsl_parse_date(s("2026-09-27"), s("yyyy-MM-dd"), s(""), &mut back), 1);
            assert_eq!(tsl_parse_date(s("nope"), s("yyyy-MM-dd"), s(""), &mut back), 0);
            assert!(tsl_make_date(2026, 2, 30, 0, 0, 0, s("")).is_nan());
        }
    }

    #[test]
    fn time_zones() {
        unsafe {
            // 2026-03-07 12:00:00 UTC.
            let t = tsl_make_date(2026, 3, 7, 12, 0, 0, s("UTC"));
            assert_eq!(t, 1772884800.0);
            let show = |zone: &str| text(tsl_format_date(t, s("yyyy-MM-dd HH:mm Z"), s(zone)));
            assert_eq!(show("UTC"), "2026-03-07 12:00 +0000");
            assert_eq!(show("Asia/Tokyo"), "2026-03-07 21:00 +0900");
            assert_eq!(show("America/New_York"), "2026-03-07 07:00 -0500");
            assert_eq!(show("+05:30"), "2026-03-07 17:30 +0530");
            assert_eq!(show("-8"), "2026-03-07 04:00 -0800");
            assert_eq!(show("Mars/Olympus"), "");
            assert_eq!(text(tsl_format_date(t, s("iso"), s("Europe/Paris"))), "2026-03-07T13:00:00+01:00");
            assert_eq!(text(tsl_format_date(t, s("iso"), s("UTC"))), "2026-03-07T12:00:00Z");
            assert_eq!(tsl_time_zone_offset(t, s("Asia/Tokyo")), 9 * 3600);
            // Summer time: New York is four hours behind in July.
            let july = tsl_make_date(2026, 7, 1, 12, 0, 0, s("UTC"));
            assert_eq!(tsl_time_zone_offset(july, s("America/New_York")), -4 * 3600);

            let mut back = 0.0;
            assert_eq!(tsl_parse_date(s("2026-03-07 21:00"), s("yyyy-MM-dd HH:mm"), s("Asia/Tokyo"), &mut back), 1);
            assert_eq!(back, t);
            // Text that says its distance from UTC doesn't need the zone.
            assert_eq!(tsl_parse_date(s("2026-03-07T13:00:00+01:00"), s("iso"), s("Asia/Tokyo"), &mut back), 1);
            assert_eq!(back, t);
            assert_eq!(tsl_parse_date(s("2026-03-07T12:00:00.5Z"), s("iso"), s(""), &mut back), 1);
            assert_eq!(back, t + 0.5);
            assert_eq!(tsl_parse_date(s("07/03/2026 07:00 -0500"), s("dd/MM/yyyy HH:mm Z"), s(""), &mut back), 1);
            assert_eq!(back, t);
            assert_eq!(tsl_parse_date(s("2026-03-07 21:00"), s("yyyy-MM-dd HH:mm"), s("Nowhere"), &mut back), 0);
            assert!(tsl_make_date(2026, 3, 7, 0, 0, 0, s("Nowhere")).is_nan());

            assert!(crate::tsl_list_len(tsl_time_zones()) > 300);
        }
    }

    #[test]
    fn calendar_arithmetic() {
        unsafe {
            let parts = |t: f64, zone: &str| -> Vec<i64> {
                let list = tsl_date_parts(t, s(zone));
                (0..8).map(|i| *(crate::tsl_list_get(list, i, std::ptr::null()) as *const i64)).collect()
            };
            let t = tsl_make_date(2026, 1, 31, 9, 30, 0, s("Europe/Paris"));
            // A Saturday, the 31st day of the year.
            assert_eq!(parts(t, "Europe/Paris"), [2026, 1, 31, 9, 30, 0, 6, 31]);
            assert_eq!(parts(t, "Asia/Tokyo")[2..5], [31, 17, 30]);
            assert_eq!(parts(f64::NAN, "UTC"), [0; 8]);
            let add = |t: f64, months: i64, days: i64, hours: i64| tsl_add_to_date(t, 0, months, days, hours, 0, 0, s("Europe/Paris"));
            // To the last day of a shorter month.
            assert_eq!(parts(add(t, 1, 0, 0), "Europe/Paris")[..5], [2026, 2, 28, 9, 30]);
            assert_eq!(parts(add(t, -2, 0, 0), "Europe/Paris")[..3], [2025, 11, 30]);
            assert_eq!(parts(tsl_add_to_date(t, 1, 1, 1, 0, 0, 0, s("Europe/Paris")), "Europe/Paris")[..3], [2027, 3, 1]);
            // Across the change to summer time (29 March), a day later is the
            // same time of day, 23 hours on; 24 hours on is an hour later.
            let before = tsl_make_date(2026, 3, 28, 9, 30, 0, s("Europe/Paris"));
            assert_eq!(add(before, 0, 1, 0) - before, 23.0 * 3600.0);
            assert_eq!(parts(add(before, 0, 0, 24), "Europe/Paris")[2..5], [29, 10, 30]);
            // A time the clocks skip.
            let skipped = tsl_make_date(2026, 3, 29, 2, 30, 0, s("Europe/Paris"));
            assert_eq!(parts(skipped, "Europe/Paris")[3..5], [3, 30]);
            assert!(add(t, 0, 0, 0).eq(&t) && tsl_add_to_date(t, 0, 0, 0, 0, 0, 0, s("Nowhere")).is_nan());

            let late = tsl_make_date(2026, 1, 31, 23, 59, 0, s("Europe/Paris"));
            assert_eq!(tsl_days_between(late, late + 120.0, s("Europe/Paris")), 1);
            assert_eq!(tsl_days_between(late, late + 120.0, s("America/New_York")), 0);
            assert_eq!(tsl_days_between(t, tsl_make_date(2025, 12, 25, 0, 0, 0, s("Europe/Paris")), s("Europe/Paris")), -37);
        }
    }
}
