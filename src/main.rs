extern crate cron;
#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

mod config;

use self::config::{Config, ConfigReader};
use anyhow::{Context, Result};
use chrono::prelude::*;
use cron::Schedule;
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::str::FromStr;
use std::thread;
use std::time::Duration;

lazy_static! {
    static ref APP_CONF: Config = ConfigReader::make();
}

#[derive(Deserialize, Debug)]
struct JenkinsJob {
    name: String,
    #[serde(rename = "lastBuild")]
    last_build: Option<LastBuild>,
}

#[derive(Deserialize, Debug)]
struct LastBuild {
    number: i64,
    url: String,
}

#[derive(Deserialize, Debug)]
struct BuildDetails {
    number: i64,
    timestamp: i64,
    result: Option<String>,
    #[serde(rename = "displayName")]
    display_name: String,
}

fn get_jenkins_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client")
}

fn check_job(job_name: &str, schedule: &Schedule, threshold_minutes: i64) -> Result<bool> {
    let now = Utc::now();
    let client = get_jenkins_client();
    
    let job_url = format!("{}/job/{}/api/json", APP_CONF.jenkins.url, job_name);
    
    debug!("Fetching job info from: {}", job_url);
    
    let response = client
        .get(&job_url)
        .basic_auth(&APP_CONF.jenkins.username, Some(&APP_CONF.jenkins.password))
        .send()
        .context("Failed to fetch job info from Jenkins")?;
    
    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Jenkins API returned error status: {}",
            response.status()
        ));
    }
    
    let job: JenkinsJob = response.json().context("Failed to parse job JSON")?;
    
    if let Some(last_build) = job.last_build {
        let build_url = format!("{}/api/json", last_build.url);
        
        debug!("Fetching build details from: {}", build_url);
        
        let build_response = client
            .get(&build_url)
            .basic_auth(&APP_CONF.jenkins.username, Some(&APP_CONF.jenkins.password))
            .send()
            .context("Failed to fetch build details")?;
        
        let build: BuildDetails = build_response.json().context("Failed to parse build JSON")?;
        
        let build_time = Utc.timestamp_millis_opt(build.timestamp).unwrap();
        let minutes_since_build = (now.timestamp() - build_time.timestamp()) / 60;
        
        info!(
            "Job '{}' - Last build #{} at {} was {:?}, {} minutes ago",
            job_name,
            build.number,
            build_time.format("%Y-%m-%d %H:%M:%S UTC"),
            build.result.as_deref().unwrap_or("RUNNING"),
            minutes_since_build
        );
        
        // Calculate when the job should have run based on the schedule
        let should_have_run = should_job_have_run(schedule, &now, threshold_minutes)?;
        
        if should_have_run && minutes_since_build > threshold_minutes {
            warn!(
                "Job '{}' hasn't run in {} minutes (threshold: {} minutes)",
                job_name, minutes_since_build, threshold_minutes
            );
            return Ok(false); // Job is overdue
        }
        
        Ok(true) // Job is running on schedule
    } else {
        warn!("Job '{}' has never been built", job_name);
        Ok(false) // No builds means alert
    }
}

fn should_job_have_run(schedule: &Schedule, now: &DateTime<Utc>, threshold_minutes: i64) -> Result<bool> {
    // Get the last scheduled time for this job
    let lookback = *now - chrono::Duration::minutes(threshold_minutes);
    
    for scheduled_time in schedule.after(&lookback).take(10) {
        if scheduled_time <= *now {
            return Ok(true);
        }
    }
    
    Ok(false)
}

fn send_email_alert(job_name: &str, message: &str) -> Result<()> {
    if let Some(email_config) = &APP_CONF.email {
        let email_body = format!(
            "Jenkins Monitor Alert\n\n\
            Job: {}\n\n\
            {}\n\n\
            Time: {}\n\
            Jenkins URL: {}\n",
            job_name,
            message,
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
            APP_CONF.jenkins.url
        );
        
        let mut email_builder = Message::builder()
            .from(email_config.from.parse()?)
            .subject(format!("Jenkins Monitor Alert: {}", job_name))
            .header(ContentType::TEXT_PLAIN);
        
        for to_addr in &email_config.to {
            email_builder = email_builder.to(to_addr.parse()?);
        }
        
        let email = email_builder.body(email_body)?;
        
        let mut mailer_builder = SmtpTransport::relay(&email_config.smtp_host)?
            .port(email_config.smtp_port);
        
        if let (Some(username), Some(password)) = (&email_config.username, &email_config.password) {
            let creds = Credentials::new(username.clone(), password.clone());
            mailer_builder = mailer_builder.credentials(creds);
        }
        
        let mailer = mailer_builder.build();
        
        match mailer.send(&email) {
            Ok(_) => {
                info!("Alert email sent for job '{}'", job_name);
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!("Failed to send email: {}", e)),
        }
    } else {
        warn!("Email not configured, skipping alert for job '{}'", job_name);
        Ok(())
    }
}

fn monitor_jobs() {
    info!("Starting job monitoring cycle...");
    
    for job_config in &APP_CONF.job {
        let schedule = match Schedule::from_str(&job_config.schedule) {
            Ok(s) => s,
            Err(e) => {
                error!("Invalid cron schedule '{}' for job '{}': {}", 
                    job_config.schedule, job_config.name, e);
                continue;
            }
        };
        
        match check_job(&job_config.name, &schedule, job_config.alert_threshold_minutes) {
            Ok(is_healthy) => {
                if !is_healthy {
                    let message = format!(
                        "Job hasn't run within expected schedule. \
                        Expected schedule: {}\n\
                        Alert threshold: {} minutes",
                        job_config.schedule, job_config.alert_threshold_minutes
                    );
                    
                    if let Err(e) = send_email_alert(&job_config.name, &message) {
                        error!("Failed to send alert for job '{}': {}", job_config.name, e);
                    }
                }
            }
            Err(e) => {
                error!("Error checking job '{}': {}", job_config.name, e);
            }
        }
    }
    
    info!("Job monitoring cycle completed");
}

fn main() {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(&APP_CONF.general.log_level))
        .init();
    
    info!("Jenkins Monitor starting...");
    info!("Monitoring {} jobs", APP_CONF.job.len());
    info!("Check interval: {} seconds", APP_CONF.general.check_interval_seconds);
    
    if APP_CONF.email.is_some() {
        info!("Email alerts enabled");
    } else {
        warn!("Email alerts not configured");
    }
    
    loop {
        monitor_jobs();
        
        debug!("Sleeping for {} seconds...", APP_CONF.general.check_interval_seconds);
        thread::sleep(Duration::from_secs(APP_CONF.general.check_interval_seconds));
    }
}
