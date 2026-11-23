//! Deciding whether a job is healthy, and when to alert about it.

use crate::config::{Config, ConfigJob};
use crate::jenkins::{job_path, JenkinsApi, JobBuilds};
use crate::notify::Notifier;
use crate::schedule::{extract_timer_spec, JobSchedule};
use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use std::collections::HashMap;

/// A build may be stamped slightly before the scheduled second (clock skew
/// between Jenkins and this host); allow this much.
const START_GRACE_SECONDS: i64 = 60;

#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    NeverBuilt {
        due: DateTime<Utc>,
    },
    Missed {
        due: DateTime<Utc>,
        last_number: i64,
        last_start: DateTime<Utc>,
    },
    Failed {
        number: i64,
        result: String,
    },
    CheckError {
        message: String,
    },
}

impl Problem {
    /// Identity used to avoid repeating an alert. A job stuck on the same
    /// last build is one problem however many runs it misses; a new failed
    /// build is a new problem.
    fn key(&self) -> String {
        match self {
            Problem::NeverBuilt { .. } => "never-built".into(),
            Problem::Missed { last_number, .. } => format!("missed-after-{}", last_number),
            Problem::Failed { number, .. } => format!("failed-{}", number),
            Problem::CheckError { .. } => "check-error".into(),
        }
    }

    fn describe(&self) -> String {
        let t = |d: &DateTime<Utc>| d.format("%Y-%m-%d %H:%M UTC").to_string();
        match self {
            Problem::NeverBuilt { due } => format!(
                "The job has never been built, but a run was scheduled for {}.",
                t(due)
            ),
            Problem::Missed {
                due,
                last_number,
                last_start,
            } => format!(
                "Missed scheduled run at {}. Last build was #{} at {}.",
                t(due),
                last_number,
                t(last_start)
            ),
            Problem::Failed { number, result } => {
                format!("Last completed build #{} finished {}.", number, result)
            }
            Problem::CheckError { message } => {
                format!("The monitor could not check this job:\n{}", message)
            }
        }
    }
}

/// Pure health check: what's wrong with this job right now?
pub fn evaluate(
    builds: &JobBuilds,
    schedule: &JobSchedule,
    threshold_minutes: i64,
    alert_on_failure: bool,
    now: DateTime<Utc>,
) -> Result<Vec<Problem>> {
    let mut problems = Vec::new();

    if let Some(due) = schedule.latest_due(now, threshold_minutes) {
        match &builds.last_build {
            None => problems.push(Problem::NeverBuilt { due }),
            Some(b) => {
                let started = b.started_at()?;
                if started < due - Duration::seconds(START_GRACE_SECONDS) {
                    problems.push(Problem::Missed {
                        due,
                        last_number: b.number,
                        last_start: started,
                    });
                }
            }
        }
    }

    if alert_on_failure {
        if let Some(c) = &builds.last_completed_build {
            if !c.succeeded() {
                problems.push(Problem::Failed {
                    number: c.number,
                    result: c.result.clone().unwrap_or_else(|| "UNKNOWN".into()),
                });
            }
        }
    }

    Ok(problems)
}

#[derive(Default, Debug)]
struct JobState {
    /// Open problems and when we last alerted about each.
    active: HashMap<String, (Problem, DateTime<Utc>)>,
    consecutive_errors: u32,
}

pub struct Monitor<J: JenkinsApi, N: Notifier> {
    cfg: Config,
    tz: Tz,
    jenkins: J,
    notifier: N,
    /// Schedules given in config.toml, parsed once at startup.
    fixed_schedules: HashMap<String, JobSchedule>,
    state: HashMap<String, JobState>,
}

impl<J: JenkinsApi, N: Notifier> Monitor<J, N> {
    pub fn new(cfg: Config, jenkins: J, notifier: N) -> Result<Self> {
        let tz = cfg.timezone()?;
        let mut fixed_schedules = HashMap::new();
        for job in &cfg.job {
            if let Some(spec) = &job.schedule {
                let s = JobSchedule::parse(spec, tz)
                    .with_context(|| format!("job '{}': bad schedule", job.name))?;
                fixed_schedules.insert(job.name.clone(), s);
            }
        }
        Ok(Monitor {
            cfg,
            tz,
            jenkins,
            notifier,
            fixed_schedules,
            state: HashMap::new(),
        })
    }

    pub fn resolve_schedule(&self, job: &ConfigJob) -> Result<JobSchedule> {
        if let Some(s) = self.fixed_schedules.get(&job.name) {
            return Ok(s.clone());
        }
        let xml = self
            .jenkins
            .job_config_xml(&job.name)
            .context("could not read the job's config.xml")?;
        let spec = extract_timer_spec(&xml).ok_or_else(|| {
            anyhow!("the job has no timer trigger in Jenkins; set `schedule` for it in config.toml")
        })?;
        JobSchedule::parse(&spec, self.tz).context("Jenkins schedule could not be read")
    }

    fn check(&self, job: &ConfigJob, now: DateTime<Utc>) -> Result<(JobSchedule, Vec<Problem>)> {
        let schedule = self.resolve_schedule(job)?;
        let builds = self
            .jenkins
            .job_builds(&job.name)
            .context("could not fetch build info")?;
        let problems = evaluate(
            &builds,
            &schedule,
            job.alert_threshold_minutes,
            job.alert_on_failure,
            now,
        )?;
        Ok((schedule, problems))
    }

    pub fn run_cycle(&mut self, now: DateTime<Utc>) {
        log::info!("Checking {} jobs", self.cfg.job.len());
        let jobs = self.cfg.job.clone();
        for job in &jobs {
            let result = self.check(job, now);
            self.handle(job, result, now);
        }
    }

    fn handle(
        &mut self,
        job: &ConfigJob,
        result: Result<(JobSchedule, Vec<Problem>)>,
        now: DateTime<Utc>,
    ) {
        let general = self.cfg.general.clone();
        let mut state = self.state.remove(&job.name).unwrap_or_default();

        let (schedule_src, current) = match result {
            Ok((schedule, problems)) => {
                state.consecutive_errors = 0;
                if problems.is_empty() {
                    log::info!("Job '{}' is healthy", job.name);
                } else {
                    for p in &problems {
                        log::warn!("Job '{}': {}", job.name, p.describe());
                    }
                }
                (Some(schedule.source().to_string()), problems)
            }
            Err(e) => {
                state.consecutive_errors += 1;
                log::error!(
                    "Job '{}': check failed ({} in a row): {:#}",
                    job.name,
                    state.consecutive_errors,
                    e
                );
                let wants_alert = job.alert_on_error.unwrap_or(general.alert_on_check_error);
                if wants_alert && state.consecutive_errors >= general.error_alert_after.max(1) {
                    // Keep whatever else was open; we just can't see it now.
                    let mut keep: Vec<Problem> = state
                        .active
                        .values()
                        .map(|(p, _)| p.clone())
                        .filter(|p| !matches!(p, Problem::CheckError { .. }))
                        .collect();
                    keep.push(Problem::CheckError {
                        message: format!("{:#}", e),
                    });
                    (None, keep)
                } else {
                    // Not alerting yet: leave state as it was.
                    self.state.insert(job.name.clone(), state);
                    return;
                }
            }
        };

        let realert = general.realert_minutes;
        let to_send: Vec<&Problem> = current
            .iter()
            .filter(|p| match state.active.get(&p.key()) {
                None => true,
                Some((_, last_sent)) => {
                    realert > 0 && now - *last_sent >= Duration::minutes(realert as i64)
                }
            })
            .collect();

        let had_problems = !state.active.is_empty();
        let mut next_active: HashMap<String, (Problem, DateTime<Utc>)> = HashMap::new();

        if !to_send.is_empty() {
            let subject = format!("Jenkins Monitor: '{}' needs attention", job.name);
            let body = self.alert_body(job, schedule_src.as_deref(), &current, now);
            match self.notifier.notify(&subject, &body) {
                Ok(()) => {
                    log::info!("Alert sent for job '{}'", job.name);
                    for p in &current {
                        next_active.insert(p.key(), (p.clone(), now));
                    }
                }
                Err(e) => {
                    // Leave unsent problems out of `active` so we retry next cycle.
                    log::error!("Failed to send alert for job '{}': {:#}", job.name, e);
                    for p in &current {
                        if let Some(prev) = state.active.get(&p.key()) {
                            next_active.insert(p.key(), (p.clone(), prev.1));
                        }
                    }
                }
            }
        } else {
            for p in &current {
                let sent = state.active.get(&p.key()).map(|(_, t)| *t).unwrap_or(now);
                next_active.insert(p.key(), (p.clone(), sent));
            }
        }

        if had_problems && current.is_empty() && general.send_recovery {
            let subject = format!("Jenkins Monitor: '{}' recovered", job.name);
            let body = format!(
                "Job '{}' is healthy again.\n\nJob: {}\nTime: {}\n",
                job.name,
                job_path(&self.cfg.jenkins.url, &job.name),
                now.format("%Y-%m-%d %H:%M:%S UTC")
            );
            if let Err(e) = self.notifier.notify(&subject, &body) {
                log::error!(
                    "Failed to send recovery notice for job '{}': {:#}",
                    job.name,
                    e
                );
            }
        }

        state.active = next_active;
        self.state.insert(job.name.clone(), state);
    }

    fn alert_body(
        &self,
        job: &ConfigJob,
        schedule: Option<&str>,
        problems: &[Problem],
        now: DateTime<Utc>,
    ) -> String {
        let mut body = format!("Jenkins Monitor alert for job '{}'\n\n", job.name);
        for p in problems {
            body.push_str("- ");
            body.push_str(&p.describe());
            body.push('\n');
        }
        body.push_str(&format!(
            "\nSchedule: {}\nTime zone: {}\nThreshold: {} minutes\nJob: {}\nChecked: {}\n",
            schedule.unwrap_or("(unknown)"),
            self.tz,
            job.alert_threshold_minutes,
            job_path(&self.cfg.jenkins.url, &job.name),
            now.format("%Y-%m-%d %H:%M:%S UTC")
        ));
        body
    }

    #[cfg(test)]
    pub fn notifier(&self) -> &N {
        &self.notifier
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jenkins::Build;
    use crate::notify::testing::RecordingNotifier;
    use chrono::TimeZone;
    use std::cell::RefCell;

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap()
    }

    fn build(number: i64, at: DateTime<Utc>, result: Option<&str>) -> Build {
        Build {
            number,
            timestamp: at.timestamp_millis(),
            result: result.map(String::from),
            url: None,
        }
    }

    fn ok_builds(number: i64, at: DateTime<Utc>) -> JobBuilds {
        let b = build(number, at, Some("SUCCESS"));
        JobBuilds {
            last_build: Some(b.clone()),
            last_completed_build: Some(b),
        }
    }

    fn daily_2am() -> JobSchedule {
        JobSchedule::parse("0 2 * * *", Tz::UTC).unwrap()
    }

    #[test]
    fn healthy_during_the_day() {
        let p = evaluate(
            &ok_builds(10, utc(2025, 12, 7, 2, 0)),
            &daily_2am(),
            90,
            true,
            utc(2025, 12, 7, 12, 0),
        )
        .unwrap();
        assert!(p.is_empty());
    }

    #[test]
    fn no_alert_before_job_had_its_chance() {
        // Old logic alerted here: at 02:01 yesterday's build looks "old".
        let p = evaluate(
            &ok_builds(9, utc(2025, 12, 6, 2, 0)),
            &daily_2am(),
            90,
            true,
            utc(2025, 12, 7, 2, 1),
        )
        .unwrap();
        assert!(p.is_empty(), "{:?}", p);
    }

    #[test]
    fn alerts_once_threshold_passes() {
        let p = evaluate(
            &ok_builds(9, utc(2025, 12, 6, 2, 0)),
            &daily_2am(),
            90,
            true,
            utc(2025, 12, 7, 3, 30),
        )
        .unwrap();
        assert!(
            matches!(p.as_slice(), [Problem::Missed { last_number: 9, .. }]),
            "{:?}",
            p
        );
    }

    #[test]
    fn late_start_within_threshold_is_fine() {
        let p = evaluate(
            &ok_builds(10, utc(2025, 12, 7, 2, 40)),
            &daily_2am(),
            90,
            true,
            utc(2025, 12, 7, 3, 30),
        )
        .unwrap();
        assert!(p.is_empty());
    }

    #[test]
    fn running_build_counts_as_started() {
        let builds = JobBuilds {
            last_build: Some(build(10, utc(2025, 12, 7, 2, 0), None)),
            last_completed_build: Some(build(9, utc(2025, 12, 6, 2, 0), Some("SUCCESS"))),
        };
        assert!(
            evaluate(&builds, &daily_2am(), 30, true, utc(2025, 12, 7, 4, 0))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn failure_is_reported_even_while_next_build_runs() {
        let builds = JobBuilds {
            last_build: Some(build(10, utc(2025, 12, 7, 2, 0), None)),
            last_completed_build: Some(build(9, utc(2025, 12, 6, 2, 0), Some("FAILURE"))),
        };
        let p = evaluate(&builds, &daily_2am(), 30, true, utc(2025, 12, 7, 4, 0)).unwrap();
        assert_eq!(
            p,
            vec![Problem::Failed {
                number: 9,
                result: "FAILURE".into()
            }]
        );
        assert!(
            evaluate(&builds, &daily_2am(), 30, false, utc(2025, 12, 7, 4, 0))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn never_built() {
        let p = evaluate(
            &JobBuilds::default(),
            &daily_2am(),
            30,
            true,
            utc(2025, 12, 7, 4, 0),
        )
        .unwrap();
        assert!(matches!(p.as_slice(), [Problem::NeverBuilt { .. }]));
    }

    // ---- alert state machine ----

    struct FakeJenkins {
        builds: RefCell<Result<JobBuilds, String>>,
    }

    impl JenkinsApi for FakeJenkins {
        fn job_builds(&self, _job: &str) -> Result<JobBuilds> {
            self.builds.borrow().clone().map_err(|e| anyhow!(e))
        }
        fn job_config_xml(&self, _job: &str) -> Result<String> {
            Ok(
                "<project><triggers><hudson.triggers.TimerTrigger><spec>0 2 * * *</spec>\
                </hudson.triggers.TimerTrigger></triggers></project>"
                    .into(),
            )
        }
    }

    fn monitor(
        extra_general: &str,
        schedule_line: &str,
    ) -> Monitor<FakeJenkins, RecordingNotifier> {
        let toml = format!(
            "[general]\n{}\n[jenkins]\nurl = \"https://ci.example\"\n[[job]]\nname = \"nightly\"\n{}\nalert_threshold_minutes = 90\n",
            extra_general, schedule_line
        );
        let cfg = Config::from_toml(&toml).unwrap();
        let jenkins = FakeJenkins {
            builds: RefCell::new(Ok(ok_builds(9, utc(2025, 12, 6, 2, 0)))),
        };
        Monitor::new(cfg, jenkins, RecordingNotifier::default()).unwrap()
    }

    fn sent(m: &Monitor<FakeJenkins, RecordingNotifier>) -> Vec<String> {
        m.notifier()
            .sent
            .borrow()
            .iter()
            .map(|(s, _)| s.clone())
            .collect()
    }

    #[test]
    fn alerts_once_then_recovers() {
        let mut m = monitor("", "schedule = \"0 2 * * *\"");
        m.run_cycle(utc(2025, 12, 7, 3, 30)); // missed
        m.run_cycle(utc(2025, 12, 7, 3, 31)); // still missed: no repeat
        m.run_cycle(utc(2025, 12, 7, 9, 0));
        assert_eq!(sent(&m).len(), 1);
        assert!(sent(&m)[0].contains("needs attention"));

        *m.jenkins.builds.borrow_mut() = Ok(ok_builds(10, utc(2025, 12, 7, 9, 5)));
        m.run_cycle(utc(2025, 12, 7, 9, 6));
        m.run_cycle(utc(2025, 12, 7, 9, 7));
        let s = sent(&m);
        assert_eq!(s.len(), 2);
        assert!(s[1].contains("recovered"));
    }

    #[test]
    fn realert_interval() {
        let mut m = monitor("realert_minutes = 60", "schedule = \"0 2 * * *\"");
        m.run_cycle(utc(2025, 12, 7, 3, 30));
        m.run_cycle(utc(2025, 12, 7, 4, 0));
        m.run_cycle(utc(2025, 12, 7, 4, 30));
        assert_eq!(sent(&m).len(), 2);
    }

    #[test]
    fn check_errors_wait_for_consecutive_failures() {
        let mut m = monitor("", "schedule = \"0 2 * * *\"");
        *m.jenkins.builds.borrow_mut() = Err("connection refused".into());
        m.run_cycle(utc(2025, 12, 7, 12, 0));
        m.run_cycle(utc(2025, 12, 7, 12, 1));
        assert!(sent(&m).is_empty());
        m.run_cycle(utc(2025, 12, 7, 12, 2));
        assert_eq!(sent(&m).len(), 1);
        assert!(m.notifier().sent.borrow()[0]
            .1
            .contains("connection refused"));
        m.run_cycle(utc(2025, 12, 7, 12, 3));
        assert_eq!(sent(&m).len(), 1);
    }

    #[test]
    fn check_error_alerts_can_be_disabled() {
        let mut m = monitor("alert_on_check_error = false", "schedule = \"0 2 * * *\"");
        *m.jenkins.builds.borrow_mut() = Err("boom".into());
        for i in 0..5 {
            m.run_cycle(utc(2025, 12, 7, 12, i));
        }
        assert!(sent(&m).is_empty());
    }

    #[test]
    fn failed_send_is_retried() {
        let mut m = monitor("", "schedule = \"0 2 * * *\"");
        *m.notifier().fail.borrow_mut() = true;
        m.run_cycle(utc(2025, 12, 7, 3, 30));
        assert!(sent(&m).is_empty());
        *m.notifier().fail.borrow_mut() = false;
        m.run_cycle(utc(2025, 12, 7, 3, 31));
        assert_eq!(sent(&m).len(), 1);
    }

    #[test]
    fn schedule_from_jenkins_config_xml() {
        let mut m = monitor("", "");
        m.run_cycle(utc(2025, 12, 7, 3, 30));
        assert_eq!(sent(&m).len(), 1);
        assert!(m.notifier().sent.borrow()[0]
            .1
            .contains("Schedule: 0 2 * * *"));
    }

    #[test]
    fn bad_fixed_schedule_fails_at_startup() {
        let cfg = Config::from_toml(
            "[general]\n[jenkins]\nurl = \"https://ci\"\n[[job]]\nname = \"x\"\nschedule = \"nope\"\n",
        )
        .unwrap();
        let j = FakeJenkins {
            builds: RefCell::new(Ok(JobBuilds::default())),
        };
        assert!(Monitor::new(cfg, j, RecordingNotifier::default()).is_err());
    }
}
