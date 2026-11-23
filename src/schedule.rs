//! Parsing of Jenkins-style trigger specs into something we can evaluate.
//!
//! Jenkins timer specs differ from the `cron` crate's syntax in ways that
//! matter for correctness:
//!
//! * Jenkins uses 5 fields (no seconds). The `cron` crate needs 6 or 7.
//! * Jenkins numbers days of the week 0-7 with 0 and 7 both Sunday; the
//!   `cron` crate numbers them 1-7 with 1 as Sunday. Passing "1" straight
//!   through turns "every Monday" into "every Sunday".
//! * Jenkins supports `H` (a per-job hashed value), `H/n`, `H(a-b)`, and
//!   aliases such as `@daily`, none of which the `cron` crate understands.
//! * A spec can hold several lines, `#` comments and a `TZ=Area/City` line.
//! * Jenkins evaluates specs in its own time zone, not UTC.
//!
//! We can't reproduce Jenkins' hash for `H`, so each `H` field is replaced by
//! the earliest value it could take, and the latest value it could take is
//! carried as extra "slack" minutes. The monitor waits for that slack before
//! deciding a run was missed, so `H` can make an alert later, never falsely.
//!
//! Specs with 6 or 7 fields are treated as `cron`-crate syntax (seconds
//! first, Sunday = 1), which is what earlier versions of this tool accepted.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use std::str::FromStr;

/// One schedule line, ready to evaluate.
#[derive(Debug, Clone)]
pub struct ScheduleEntry {
    schedule: Schedule,
    tz: Tz,
    /// Extra minutes the real run may come after the computed occurrence,
    /// because of `H` fields.
    slack_minutes: i64,
}

/// A whole trigger spec: every line Jenkins would fire on.
#[derive(Debug, Clone)]
pub struct JobSchedule {
    source: String,
    entries: Vec<ScheduleEntry>,
}

impl JobSchedule {
    /// Parse a spec as found in config.toml or a Jenkins `config.xml`.
    /// `default_tz` is the Jenkins server's time zone, used unless the spec
    /// contains its own `TZ=` line.
    pub fn parse(spec: &str, default_tz: Tz) -> Result<Self> {
        let mut tz = default_tz;
        let mut entries = Vec::new();

        for raw_line in spec.lines() {
            let line = match raw_line.find('#') {
                Some(i) => &raw_line[..i],
                None => raw_line,
            }
            .trim();
            if line.is_empty() {
                continue;
            }

            if let Some(zone) = line.strip_prefix("TZ=") {
                tz = Tz::from_str(zone.trim()).map_err(|e| {
                    anyhow!("unknown time zone '{}' in schedule: {}", zone.trim(), e)
                })?;
                continue;
            }

            let (expr, slack_minutes) = to_cron_expression(line)
                .with_context(|| format!("cannot read schedule line '{}'", line))?;
            let schedule = Schedule::from_str(&expr)
                .with_context(|| format!("invalid cron expression '{}' (from '{}')", expr, line))?;
            entries.push(ScheduleEntry {
                schedule,
                tz,
                slack_minutes,
            });
        }

        if entries.is_empty() {
            bail!("schedule '{}' has no cron lines", spec.trim());
        }

        Ok(JobSchedule {
            source: spec.trim().to_string(),
            entries,
        })
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The most recent scheduled run whose grace period (`threshold_minutes`
    /// plus any `H` slack) has fully passed by `now`. A build that started at
    /// or after the returned time satisfies the schedule; if the last build is
    /// older than it, that run was missed. Returns `None` when no run is due
    /// yet (for example a schedule that only fires in the future).
    pub fn latest_due(&self, now: DateTime<Utc>, threshold_minutes: i64) -> Option<DateTime<Utc>> {
        self.entries
            .iter()
            .filter_map(|e| {
                let cutoff = now - Duration::minutes(threshold_minutes + e.slack_minutes);
                // `after(x).rev()` yields occurrences strictly before x; nudge
                // by a second so an occurrence exactly at the cutoff counts.
                let probe = (cutoff + Duration::seconds(1)).with_timezone(&e.tz);
                e.schedule
                    .after(&probe)
                    .next_back()
                    .map(|t| t.with_timezone(&Utc))
            })
            .max()
    }
}

/// Convert one spec line to a `cron`-crate expression and its `H` slack.
fn to_cron_expression(line: &str) -> Result<(String, i64)> {
    let expanded = match line {
        "@yearly" | "@annually" => "H H H H *",
        "@monthly" => "H H H * *",
        "@weekly" => "H H * * H",
        "@daily" => "H H * * *",
        "@midnight" => "H H(0-2) * * *",
        "@hourly" => "H * * * *",
        other if other.starts_with('@') => bail!("unsupported alias '{}'", other),
        other => other,
    };

    let fields: Vec<&str> = expanded.split_whitespace().collect();
    match fields.len() {
        5 => jenkins_fields_to_cron(&fields),
        6 | 7 => {
            if fields.iter().any(|f| f.contains('H')) {
                bail!("'H' is only supported in 5-field Jenkins specs");
            }
            Ok((fields.join(" "), 0))
        }
        n => bail!(
            "expected 5 fields (Jenkins) or 6-7 fields (with seconds), found {}",
            n
        ),
    }
}

#[derive(Clone, Copy)]
struct FieldKind {
    min: u32,
    max: u32,
    /// Range Jenkins draws plain `H` from (day of month uses 1-28).
    hash_max: u32,
    unit_minutes: i64,
}

const MINUTE: FieldKind = FieldKind {
    min: 0,
    max: 59,
    hash_max: 59,
    unit_minutes: 1,
};
const HOUR: FieldKind = FieldKind {
    min: 0,
    max: 23,
    hash_max: 23,
    unit_minutes: 60,
};
const DOM: FieldKind = FieldKind {
    min: 1,
    max: 31,
    hash_max: 28,
    unit_minutes: 1440,
};
const MONTH: FieldKind = FieldKind {
    min: 1,
    max: 12,
    hash_max: 12,
    unit_minutes: 31 * 1440,
};
const DOW: FieldKind = FieldKind {
    min: 0,
    max: 6,
    hash_max: 6,
    unit_minutes: 1440,
};

fn jenkins_fields_to_cron(fields: &[&str]) -> Result<(String, i64)> {
    let kinds = [MINUTE, HOUR, DOM, MONTH, DOW];
    let mut out = vec!["0".to_string()]; // seconds
    let mut slack = 0i64;

    for (i, (field, kind)) in fields.iter().zip(kinds.iter()).enumerate() {
        let (expanded, field_slack) = expand_hash(field, *kind)?;
        slack += field_slack;
        let converted = if i == 4 {
            convert_dow(&expanded)?
        } else {
            expanded
        };
        out.push(converted);
    }

    Ok((out.join(" "), slack))
}

/// Replace `H` forms in one field with their earliest value. Returns the new
/// field text and the slack in minutes.
fn expand_hash(field: &str, kind: FieldKind) -> Result<(String, i64)> {
    if !field.contains('H') {
        return Ok((field.to_string(), 0));
    }

    let mut parts = Vec::new();
    let mut slack_units = 0u32;

    for part in field.split(',') {
        let (base, step) = match part.split_once('/') {
            Some((b, s)) => {
                let s: u32 = s
                    .parse()
                    .with_context(|| format!("bad step in '{}'", part))?;
                if s == 0 {
                    bail!("step of zero in '{}'", part);
                }
                (b, Some(s))
            }
            None => (part, None),
        };

        if !base.starts_with('H') {
            parts.push(part.to_string());
            continue;
        }

        let (lo, hi) = if base == "H" {
            (
                kind.min,
                if step.is_some() {
                    kind.max
                } else {
                    kind.hash_max
                },
            )
        } else if let Some(range) = base.strip_prefix("H(").and_then(|r| r.strip_suffix(')')) {
            let (a, b) = range
                .split_once('-')
                .ok_or_else(|| anyhow!("expected H(a-b), found '{}'", base))?;
            let a: u32 = a
                .trim()
                .parse()
                .with_context(|| format!("bad range in '{}'", base))?;
            let b: u32 = b
                .trim()
                .parse()
                .with_context(|| format!("bad range in '{}'", base))?;
            if a > b || a < kind.min || b > kind.max {
                bail!("range out of bounds in '{}'", base);
            }
            (a, b)
        } else {
            bail!("unsupported hash form '{}'", base);
        };

        match step {
            Some(s) => {
                parts.push(format!("{}-{}/{}", lo, hi, s));
                slack_units = slack_units.max((s - 1).min(hi - lo));
            }
            None => {
                parts.push(lo.to_string());
                slack_units = slack_units.max(hi - lo);
            }
        }
    }

    Ok((parts.join(","), slack_units as i64 * kind.unit_minutes))
}

/// Convert a Jenkins day-of-week field (0-7, Sunday = 0 or 7) to the `cron`
/// crate's numbering (1-7, Sunday = 1). Names such as MON pass through.
fn convert_dow(field: &str) -> Result<String> {
    let mut out = Vec::new();
    for part in field.split(',') {
        let (base, step) = match part.split_once('/') {
            Some((b, s)) => (b, Some(s)),
            None => (part, None),
        };
        let with_step = |b: String| match step {
            Some(s) => format!("{}/{}", b, s),
            None => b,
        };

        if base == "*" || base == "?" || base.chars().any(|c| c.is_ascii_alphabetic()) {
            out.push(part.to_string());
            continue;
        }

        let num = |s: &str| -> Result<u32> {
            let n: u32 = s
                .parse()
                .with_context(|| format!("bad day of week '{}'", s))?;
            if n > 7 {
                bail!("day of week {} out of range 0-7", n);
            }
            Ok(n)
        };

        match base.split_once('-') {
            Some((a, b)) => {
                let (a, b) = (num(a)?, num(b)?);
                if a > b {
                    bail!("descending day-of-week range '{}'", base);
                }
                if b == 7 && a > 0 {
                    // e.g. 5-7 (Fri-Sun): Fri-Sat plus Sunday
                    if step.is_some() {
                        bail!(
                            "stepped day-of-week range ending in 7 is not supported: '{}'",
                            part
                        );
                    }
                    out.push(format!("{}-7", a + 1));
                    out.push("1".to_string());
                } else if b == 7 {
                    out.push(with_step("1-7".to_string())); // 0-7 is every day
                } else {
                    out.push(with_step(format!("{}-{}", a + 1, b + 1)));
                }
            }
            None => {
                let n = num(base)?;
                let mapped = if n == 7 { 1 } else { n + 1 };
                out.push(with_step(mapped.to_string()));
            }
        }
    }
    Ok(out.join(","))
}

/// Pull the timer trigger spec out of a Jenkins job `config.xml`.
///
/// Only `hudson.triggers.TimerTrigger` counts. An SCM polling spec
/// (`SCMTrigger`) is not a promise to build, so it is deliberately ignored.
pub fn extract_timer_spec(config_xml: &str) -> Option<String> {
    let start = config_xml.find("<hudson.triggers.TimerTrigger")?;
    let rest = &config_xml[start..];
    let end = rest
        .find("</hudson.triggers.TimerTrigger>")
        .unwrap_or(rest.len());
    let block = &rest[..end];

    let spec_start = block.find("<spec>")? + "<spec>".len();
    let spec_end = block[spec_start..].find("</spec>")? + spec_start;
    let spec = decode_xml_entities(&block[spec_start..spec_end]);
    let spec = spec.trim();
    if spec.is_empty() {
        None
    } else {
        Some(spec.to_string())
    }
}

fn decode_xml_entities(s: &str) -> String {
    s.replace("&#xd;", "")
        .replace("&#xD;", "")
        .replace("&#13;", "")
        .replace("&#xa;", "\n")
        .replace("&#10;", "\n")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone, Weekday};

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn parse(spec: &str) -> JobSchedule {
        JobSchedule::parse(spec, Tz::UTC).unwrap()
    }

    #[test]
    fn jenkins_monday_means_monday() {
        let s = parse("0 9 * * 1");
        // Wed 2025-12-10 -> last Monday 09:00 is 2025-12-08
        let due = s.latest_due(utc(2025, 12, 10, 12, 0), 0).unwrap();
        assert_eq!(due.weekday(), Weekday::Mon);
        assert_eq!(due, utc(2025, 12, 8, 9, 0));
    }

    #[test]
    fn jenkins_sunday_zero_and_seven() {
        for spec in ["0 9 * * 0", "0 9 * * 7"] {
            let due = parse(spec).latest_due(utc(2025, 12, 10, 12, 0), 0).unwrap();
            assert_eq!(due.weekday(), Weekday::Sun, "spec {}", spec);
        }
    }

    #[test]
    fn jenkins_weekday_ranges() {
        assert_eq!(convert_dow("1-5").unwrap(), "2-6");
        assert_eq!(convert_dow("0-6").unwrap(), "1-7");
        assert_eq!(convert_dow("5-7").unwrap(), "6-7,1");
        assert_eq!(convert_dow("MON-FRI").unwrap(), "MON-FRI");
        assert_eq!(convert_dow("*/2").unwrap(), "*/2");
        assert_eq!(convert_dow("1,3").unwrap(), "2,4");
        assert!(convert_dow("8").is_err());
    }

    #[test]
    fn six_field_specs_keep_old_meaning() {
        let s = parse("0 0 2 * * *");
        assert_eq!(
            s.latest_due(utc(2025, 12, 7, 12, 0), 90),
            Some(utc(2025, 12, 7, 2, 0))
        );
    }

    #[test]
    fn due_waits_for_threshold() {
        let s = parse("0 2 * * *");
        // 02:30 with a 90 minute threshold: today's 02:00 run isn't due yet,
        // yesterday's is.
        assert_eq!(
            s.latest_due(utc(2025, 12, 7, 2, 30), 90),
            Some(utc(2025, 12, 6, 2, 0))
        );
        // Exactly at 03:30 today's run becomes due.
        assert_eq!(
            s.latest_due(utc(2025, 12, 7, 3, 30), 90),
            Some(utc(2025, 12, 7, 2, 0))
        );
    }

    #[test]
    fn due_found_for_long_intervals() {
        // Monthly schedule, small threshold: the old fixed look-back window
        // couldn't find an occurrence and errored on most days.
        let s = parse("0 3 1 * *");
        assert_eq!(
            s.latest_due(utc(2025, 12, 20, 0, 0), 30),
            Some(utc(2025, 12, 1, 3, 0))
        );
    }

    #[test]
    fn hash_adds_slack_not_false_alarms() {
        let (expr, slack) = to_cron_expression("H 2 * * *").unwrap();
        assert_eq!(expr, "0 0 2 * * *");
        assert_eq!(slack, 59);

        let (expr, slack) = to_cron_expression("H/15 * * * *").unwrap();
        assert_eq!(expr, "0 0-59/15 * * * *");
        assert_eq!(slack, 14);

        let (expr, slack) = to_cron_expression("H(0-29) H(1-3) * * *").unwrap();
        assert_eq!(expr, "0 0 1 * * *");
        assert_eq!(slack, 29 + 2 * 60);

        // H 2 * * * with threshold 30: a build at 02:45 is legal, so at 03:00
        // the run must not be due yet.
        let s = parse("H 2 * * *");
        assert_eq!(
            s.latest_due(utc(2025, 12, 7, 3, 0), 30),
            Some(utc(2025, 12, 6, 2, 0))
        );
        assert_eq!(
            s.latest_due(utc(2025, 12, 7, 3, 29), 30),
            Some(utc(2025, 12, 7, 2, 0))
        );
    }

    #[test]
    fn aliases() {
        assert!(JobSchedule::parse("@daily", Tz::UTC).is_ok());
        assert!(JobSchedule::parse("@hourly", Tz::UTC).is_ok());
        assert!(JobSchedule::parse("@midnight", Tz::UTC).is_ok());
        assert!(JobSchedule::parse("@weekly", Tz::UTC).is_ok());
        assert!(JobSchedule::parse("@reboot", Tz::UTC).is_err());
    }

    #[test]
    fn multi_line_comments_and_tz() {
        let spec = "# nightly\nTZ=America/New_York\n0 2 * * *\n\n0 14 * * *  # afternoon\n";
        let s = JobSchedule::parse(spec, Tz::UTC).unwrap();
        // 02:00 New York (EST, UTC-5) = 07:00 UTC; 14:00 NY = 19:00 UTC.
        assert_eq!(
            s.latest_due(utc(2025, 12, 7, 20, 0), 0),
            Some(utc(2025, 12, 7, 19, 0))
        );
        assert_eq!(
            s.latest_due(utc(2025, 12, 7, 12, 0), 0),
            Some(utc(2025, 12, 7, 7, 0))
        );
    }

    #[test]
    fn server_time_zone_is_used() {
        let ny: Tz = "America/New_York".parse().unwrap();
        let s = JobSchedule::parse("0 2 * * *", ny).unwrap();
        assert_eq!(
            s.latest_due(utc(2025, 12, 7, 12, 0), 0),
            Some(utc(2025, 12, 7, 7, 0))
        );
    }

    #[test]
    fn rejects_bad_specs() {
        assert!(JobSchedule::parse("", Tz::UTC).is_err());
        assert!(JobSchedule::parse("# only a comment", Tz::UTC).is_err());
        assert!(JobSchedule::parse("1 2 3", Tz::UTC).is_err());
        assert!(JobSchedule::parse("TZ=Mars/Olympus\n0 2 * * *", Tz::UTC).is_err());
        assert!(JobSchedule::parse("0 0 H * * *", Tz::UTC).is_err());
    }

    #[test]
    fn extracts_timer_trigger_only() {
        let xml = r#"<project><triggers>
            <hudson.triggers.SCMTrigger><spec>H/5 * * * *</spec></hudson.triggers.SCMTrigger>
            <hudson.triggers.TimerTrigger><spec>H 2 * * *&#xd;
TZ=Europe/London</spec></hudson.triggers.TimerTrigger>
        </triggers></project>"#;
        assert_eq!(
            extract_timer_spec(xml).as_deref(),
            Some("H 2 * * *\nTZ=Europe/London")
        );
    }

    #[test]
    fn scm_polling_alone_is_not_a_schedule() {
        let xml = "<project><triggers><hudson.triggers.SCMTrigger><spec>H/15 * * * *</spec>\
                   </hudson.triggers.SCMTrigger></triggers></project>";
        assert_eq!(extract_timer_spec(xml), None);
    }

    #[test]
    fn pipeline_job_timer_trigger() {
        let xml = r#"<flow-definition plugin="workflow-job@1400">
          <properties><org.jenkinsci.plugins.workflow.job.properties.PipelineTriggersJobProperty>
            <triggers><hudson.triggers.TimerTrigger><spec>@daily</spec></hudson.triggers.TimerTrigger></triggers>
          </org.jenkinsci.plugins.workflow.job.properties.PipelineTriggersJobProperty></properties>
        </flow-definition>"#;
        assert_eq!(extract_timer_spec(xml).as_deref(), Some("@daily"));
    }
}
