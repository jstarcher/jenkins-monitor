use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;

use crate::alert::EmailAlerter;
use crate::config::{Config, JobConfig};
use crate::jenkins::{JenkinsClient, LastBuildInfo};

pub struct Monitor {
    config: Config,
    jenkins_client: JenkinsClient,
    email_alerter: Option<EmailAlerter>,
    job_states: HashMap<String, JobState>,
}

#[derive(Debug, Clone)]
struct JobState {
    last_check: DateTime<Utc>,
    last_build_info: Option<LastBuildInfo>,
    last_alert_sent: Option<DateTime<Utc>>,
}

impl Monitor {
    pub fn new(config: Config, jenkins_client: JenkinsClient) -> Self {
        let email_alerter = config.alerts.email.as_ref().map(|email_config| {
            EmailAlerter::new(email_config.clone())
        });
        
        let job_states = HashMap::new();
        
        Self {
            config,
            jenkins_client,
            email_alerter,
            job_states,
        }
    }
    
    pub async fn run(mut self) -> Result<()> {
        // Test Jenkins connection first
        log::info!("Testing Jenkins connection...");
        self.jenkins_client.test_connection().await?;
        log::info!("Jenkins connection successful");
        
        let check_interval = tokio::time::Duration::from_secs(60);
        let mut interval = tokio::time::interval(check_interval);
        
        loop {
            interval.tick().await;
            
            log::info!("Running monitoring check...");
            
            if let Err(e) = self.check_all_jobs().await {
                log::error!("Error during monitoring check: {}", e);
            }
        }
    }
    
    async fn check_all_jobs(&mut self) -> Result<()> {
        let now = Utc::now();
        
        // Clone the job configs to avoid borrow issues
        let jobs: Vec<JobConfig> = self.config.jobs.clone();
        
        for job_config in jobs {
            if !job_config.enabled {
                log::debug!("Skipping disabled job: {}", job_config.name);
                continue;
            }
            
            if let Err(e) = self.check_job(&job_config, now).await {
                log::error!("Error checking job '{}': {}", job_config.name, e);
            }
        }
        
        Ok(())
    }
    
    async fn check_job(&mut self, job_config: &JobConfig, now: DateTime<Utc>) -> Result<()> {
        log::info!("Checking job: {}", job_config.name);
        
        // Get current build info from Jenkins
        let current_build = self.jenkins_client.get_last_build(&job_config.name).await?;
        
        // Check if job should have run
        let expected_run_time = self.calculate_expected_run_time(&job_config.expected_schedule, now)?;
        let threshold = Duration::minutes(job_config.alert_threshold_mins as i64);
        
        // Get or create job state
        let state = self.job_states.entry(job_config.name.clone()).or_insert_with(|| {
            JobState {
                last_check: now,
                last_build_info: None,
                last_alert_sent: None,
            }
        });
        
        // If we have a last build, check if it's recent enough
        let should_alert = if let Some(ref build_info) = current_build {
            let time_since_expected = now.signed_duration_since(expected_run_time);
            
            log::debug!(
                "Job '{}': last build at {}, expected at {}, threshold {} mins",
                job_config.name,
                build_info.timestamp,
                expected_run_time,
                job_config.alert_threshold_mins
            );
            
            // Alert if the job hasn't run since the expected time + threshold
            if time_since_expected > threshold && build_info.timestamp < expected_run_time {
                log::warn!(
                    "Job '{}' hasn't run since expected time. Last build: {}, Expected: {}",
                    job_config.name,
                    build_info.timestamp,
                    expected_run_time
                );
                true
            } else {
                false
            }
        } else {
            // No builds found - alert if we're past the expected time + threshold
            let time_since_expected = now.signed_duration_since(expected_run_time);
            if time_since_expected > threshold {
                log::warn!("Job '{}' has no builds and is past expected run time", job_config.name);
                true
            } else {
                false
            }
        };
        
        // Check if we should send an alert (not sent recently)
        let should_send_alert = if should_alert {
            state.last_alert_sent
                .map(|last| now.signed_duration_since(last) > Duration::hours(1))
                .unwrap_or(true)
        } else {
            false
        };
        
        // Update state
        state.last_check = now;
        state.last_build_info = current_build.clone();
        
        // Send alert if needed
        if should_send_alert {
            self.send_alert(job_config, &current_build, expected_run_time, now).await?;
            // Update last alert time
            if let Some(state) = self.job_states.get_mut(&job_config.name) {
                state.last_alert_sent = Some(now);
            }
        } else if should_alert {
            log::debug!("Alert suppressed for job '{}' - already sent recently", job_config.name);
        }
        
        Ok(())
    }
    
    fn calculate_expected_run_time(&self, cron_expr: &str, now: DateTime<Utc>) -> Result<DateTime<Utc>> {
        use cron::Schedule;
        use std::str::FromStr;
        
        let schedule = Schedule::from_str(cron_expr)?;
        
        // Find the most recent expected run time before now
        let mut expected_time = now;
        for upcoming in schedule.upcoming(Utc).take(10) {
            if upcoming > now {
                break;
            }
            expected_time = upcoming;
        }
        
        // If we couldn't find a recent time, find the last time before now
        if expected_time == now {
            // Go back in time to find the last expected run
            let past_time = now - Duration::days(7);
            for upcoming in schedule.after(&past_time).take(1000) {
                if upcoming > now {
                    break;
                }
                expected_time = upcoming;
            }
        }
        
        Ok(expected_time)
    }
    
    async fn send_alert(
        &self,
        job_config: &JobConfig,
        last_build: &Option<LastBuildInfo>,
        expected_time: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let subject = format!("Jenkins Job Alert: {}", job_config.name);
        
        let body = if let Some(build) = last_build {
            format!(
                "Jenkins Monitor Alert\n\n\
                Job: {}\n\
                Status: Job has not run as expected\n\n\
                Expected Schedule: {}\n\
                Last Expected Run: {}\n\
                Last Build: {} (Build #{})\n\
                Build Result: {}\n\
                Time Since Last Build: {} minutes\n\
                Alert Threshold: {} minutes\n\n\
                The job has not run since the expected time plus the configured threshold.\n\
                Please check Jenkins for issues.\n\n\
                Jenkins URL: {}/job/{}",
                job_config.name,
                job_config.expected_schedule,
                expected_time.format("%Y-%m-%d %H:%M:%S UTC"),
                build.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
                build.number,
                build.result.as_deref().unwrap_or("UNKNOWN"),
                now.signed_duration_since(build.timestamp).num_minutes(),
                job_config.alert_threshold_mins,
                self.config.jenkins.url,
                job_config.name
            )
        } else {
            format!(
                "Jenkins Monitor Alert\n\n\
                Job: {}\n\
                Status: No builds found\n\n\
                Expected Schedule: {}\n\
                Last Expected Run: {}\n\
                Last Build: None\n\
                Alert Threshold: {} minutes\n\n\
                The job has no build history and should have run by now.\n\
                Please check Jenkins for issues.\n\n\
                Jenkins URL: {}/job/{}",
                job_config.name,
                job_config.expected_schedule,
                expected_time.format("%Y-%m-%d %H:%M:%S UTC"),
                job_config.alert_threshold_mins,
                self.config.jenkins.url,
                job_config.name
            )
        };
        
        if let Some(alerter) = &self.email_alerter {
            alerter.send_alert(&subject, &body)?;
        } else {
            log::warn!("No email alerter configured - alert would have been sent:");
            log::warn!("Subject: {}", subject);
            log::warn!("Body:\n{}", body);
        }
        
        Ok(())
    }
}
