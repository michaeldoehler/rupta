//! Expected time of a monitor's next check-in, from the schedule the SDK
//! sends in `monitor_config`: a five-field crontab (`*`, lists, ranges,
//! steps, month and weekday names) or an interval. Crontabs are evaluated
//! in UTC; other time zones are stored but not applied.

use chrono::{DateTime, Datelike, Duration, Timelike, Utc};
use serde_json::Value;

/// The next run after `after`, or `None` for schedules not understood.
pub fn next_run(schedule: &Value, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match schedule.get("type").and_then(Value::as_str)? {
        "crontab" => Crontab::parse(schedule.get("value")?.as_str()?)?.next_after(after),
        "interval" => {
            let n = schedule.get("value")?.as_i64().filter(|n| *n > 0)?;
            let unit = schedule.get("unit").and_then(Value::as_str).unwrap_or("minute");
            let d = match unit.trim_end_matches('s') {
                "minute" => Duration::minutes(n),
                "hour" => Duration::hours(n),
                "day" => Duration::days(n),
                "week" => Duration::weeks(n),
                "month" => Duration::days(30 * n),
                "year" => Duration::days(365 * n),
                _ => return None,
            };
            Some(after + d)
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Crontab {
    minutes: Vec<bool>,
    hours: Vec<bool>,
    days: Vec<bool>,
    months: Vec<bool>,
    weekdays: Vec<bool>,
    day_any: bool,
    weekday_any: bool,
}

fn names(field: &str, names: &[&str], offset: u32) -> String {
    let mut f = field.to_ascii_lowercase();
    for (i, n) in names.iter().enumerate() {
        f = f.replace(n, &(i as u32 + offset).to_string());
    }
    f
}

fn field(spec: &str, min: u32, max: u32) -> Option<Vec<bool>> {
    let mut set = vec![false; max as usize + 1];
    for part in spec.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().ok().filter(|s| *s > 0)?),
            None => (part, 1),
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (a.parse().ok()?, b.parse().ok()?)
        } else {
            let v: u32 = range.parse().ok()?;
            (v, if part.contains('/') { max } else { v })
        };
        if lo < min || hi > max || lo > hi {
            return None;
        }
        let mut v = lo;
        while v <= hi {
            set[v as usize] = true;
            v += step;
        }
    }
    Some(set)
}

impl Crontab {
    pub fn parse(expr: &str) -> Option<Crontab> {
        let expr = match expr.trim() {
            "@yearly" | "@annually" => "0 0 1 1 *",
            "@monthly" => "0 0 1 * *",
            "@weekly" => "0 0 * * 0",
            "@daily" | "@midnight" => "0 0 * * *",
            "@hourly" => "0 * * * *",
            other => other,
        };
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return None;
        }
        let months =
            names(parts[3], &["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"], 1);
        let weekdays = names(parts[4], &["sun", "mon", "tue", "wed", "thu", "fri", "sat"], 0);
        let mut wd = field(&weekdays, 0, 7)?;
        if wd[7] {
            wd[0] = true;
        }
        Some(Crontab {
            minutes: field(parts[0], 0, 59)?,
            hours: field(parts[1], 0, 23)?,
            days: field(parts[2], 1, 31)?,
            months: field(&months, 1, 12)?,
            weekdays: wd,
            day_any: parts[2] == "*",
            weekday_any: parts[4] == "*",
        })
    }

    fn day_matches(&self, t: DateTime<Utc>) -> bool {
        let dom = self.days[t.day() as usize];
        let dow = self.weekdays[t.weekday().num_days_from_sunday() as usize];
        // Standard cron: if both are restricted, either may match.
        match (self.day_any, self.weekday_any) {
            (true, true) => true,
            (true, false) => dow,
            (false, true) => dom,
            (false, false) => dom || dow,
        }
    }

    /// The first matching minute strictly after `after` (within 5 years).
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut t = after.with_second(0)?.with_nanosecond(0)? + Duration::minutes(1);
        let limit = after + Duration::days(366 * 5);
        while t < limit {
            if !self.months[t.month() as usize] {
                t = (t + Duration::days(32 - t.day() as i64)).with_day(1)?.with_hour(0)?.with_minute(0)?;
                continue;
            }
            if !self.day_matches(t) {
                t = (t + Duration::days(1)).with_hour(0)?.with_minute(0)?;
                continue;
            }
            if !self.hours[t.hour() as usize] {
                t = (t + Duration::hours(1)).with_minute(0)?;
                continue;
            }
            if !self.minutes[t.minute() as usize] {
                t += Duration::minutes(1);
                continue;
            }
            return Some(t);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    #[test]
    fn crontab_schedules() {
        let t = at(2026, 11, 14, 10, 7);
        let next = |e: &str| next_run(&json!({"type": "crontab", "value": e}), t);
        assert_eq!(next("*/15 * * * *"), Some(at(2026, 11, 14, 10, 15)));
        assert_eq!(next("0 * * * *"), Some(at(2026, 11, 14, 11, 0)));
        assert_eq!(next("30 2 * * *"), Some(at(2026, 11, 15, 2, 30)));
        assert_eq!(next("0 9 * * mon-fri"), Some(at(2026, 11, 16, 9, 0))); // 14th is a Saturday
        assert_eq!(next("0 0 1 jan *"), Some(at(2027, 1, 1, 0, 0)));
        assert_eq!(next("@daily"), Some(at(2026, 11, 15, 0, 0)));
        assert_eq!(next("0 0 31 2 *"), None);
        assert_eq!(next("bogus"), None);
        assert_eq!(next("61 * * * *"), None);
    }

    #[test]
    fn intervals() {
        let t = at(2026, 11, 14, 10, 0);
        assert_eq!(
            next_run(&json!({"type": "interval", "value": 2, "unit": "hour"}), t),
            Some(at(2026, 11, 14, 12, 0))
        );
        assert_eq!(
            next_run(&json!({"type": "interval", "value": 10, "unit": "minutes"}), t),
            Some(at(2026, 11, 14, 10, 10))
        );
        assert_eq!(next_run(&json!({"type": "interval", "value": 0, "unit": "hour"}), t), None);
    }
}
