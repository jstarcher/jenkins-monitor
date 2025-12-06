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
    #[allow(dead_code)]
    name: String,
    #[serde(rename = "lastBuild")]
    last_build: Option<LastBuild>,
}

#[derive(Deserialize, Debug)]
struct LastBuild {
    #[allow(dead_code)]
    number: i64,
    url: String,
}

#[derive(Deserialize, Debug)]
struct BuildDetails {
    number: i64,
    timestamp: i64,
    result: Option<String>,
    #[serde(rename = "displayName")]
    #[allow(dead_code)]
    display_name: String,
}

fn get_jenkins_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client")
}

// Build a Jenkins job API URL that supports nested job paths
// Jenkins expects nested jobs to use repeated `/job/{name}` segments,
// e.g. for "folder/subfolder/jobname" => /job/folder/job/subfolder/job/jobname/api/json
fn build_job_api_url(base_url: &str, job_name: &str) -> String {
    let mut url = base_url.trim_end_matches('/').to_string();

    for part in job_name.split('/') {
        let enc = urlencoding::encode(part);
        url.push_str(&format!("/job/{}", enc));
    }

    url.push_str("/api/json");
    url
}

fn check_job(job_name: &str, schedule: &Schedule, threshold_minutes: i64) -> Result<bool> {
    let now = Utc::now();
    let client = get_jenkins_client();
    
    let job_url = build_job_api_url(&APP_CONF.jenkins.url, job_name);
    
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
        // Construct a safe build API URL from the build's URL returned by
        // Jenkins. Jenkins may return unencoded or odd-looking URLs (e.g.
        // spaces in folder names), so parse + join to produce a valid
        // `.../api/json` endpoint.
        let build_url = build_api_url_from_last_build(&last_build.url, &APP_CONF.jenkins.url)
            .context("Failed to construct build API URL from Jenkins returned url")?;

        debug!("Fetching build details from: {}", build_url);
        
        let build_response = client
            .get(&build_url)
            .basic_auth(&APP_CONF.jenkins.username, Some(&APP_CONF.jenkins.password))
            .send()
            .context("Failed to fetch build details")?;
        
        if !build_response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Jenkins build API returned error status: {}",
                build_response.status()
            ));
        }

        let build: BuildDetails = build_response.json().context("Failed to parse build JSON")?;
        
        let build_time = Utc
            .timestamp_millis_opt(build.timestamp)
            .single()
            .context("Invalid timestamp from Jenkins")?;
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

/// Create a safe Jenkins build API URL from the `lastBuild.url` value returned
/// by the Jenkins API. This ensures the returned string points to the
/// `.../api/json` endpoint and tolerates common small issues such as a
/// missing trailing slash or unencoded spaces in path segments.
/// Build the API URL for the build by using the `lastBuild.url` returned by
/// Jenkins whenever possible. If `lastBuild.url` points to a different host
/// than the configured Jenkins base URL, prefer the configured base host
/// but reuse the path from `lastBuild.url`.
fn build_api_url_from_last_build(raw: &str, configured_base: &str) -> Result<String> {
    // Ensure there's a trailing slash so `join("api/json")` appends correctly.
    let mut base = raw.to_string();
    if !base.ends_with('/') {
        base.push('/');
    }

    // First try parsing as-is. If parsing fails (e.g., due to spaces), try a
    // best-effort percent-encoding for spaces and re-parse. We intentionally
    // only escape spaces here rather than aggressively encoding the whole URL
    // because encoding the scheme/host would break the URL.
    // Try to parse the configured base now — we'll use it when hosts differ so
    // we can reuse the build path with the canonical host. Keep a non-fatal
    // parsed value (Option) so we can log helpful debug messages; when the
    // configured base is required to build the final URL we will parse it with
    // context to report a helpful error if it's invalid.
    let cfg_parsed = url::Url::parse(configured_base).ok();

    match url::Url::parse(&base) {
        Ok(u) => {
            // If the host matches configured_base, use the returned URL as-is.
            if let Ok(cfg) = url::Url::parse(configured_base) {
                if u.scheme() == cfg.scheme() && u.host_str() == cfg.host_str() {
                    return Ok(u.join("api/json").context("Failed to append api/json to URL")?.into());
                }
            }

            // Hosts differ — fallthrough to use the returned path with the
            // configured base host (below)
            let path = u.path();
            debug!("Build URL host `{}` differs from configured host `{}` — using configured host and build path {}", u.host_str().unwrap_or(""), cfg_parsed.as_ref().and_then(|c| c.host_str()).unwrap_or(""), path);
            let mut safe_path = path.to_string();
            if !safe_path.ends_with('/') {
                safe_path.push('/');
            }

            // Try parsing the configured base and join the path + api/json
            let cfg = url::Url::parse(configured_base).context("Invalid configured Jenkins base URL")?;
            let joined = cfg.join(&safe_path).context("Failed to join configured base with build path")?;
            return Ok(joined.join("api/json").context("Failed to append api/json to URL")?.into());
        }
        Err(_) => {
            let safe = base.replace(' ', "%20");
            let parsed = url::Url::parse(&safe).context("Invalid build URL from Jenkins")?;

            if let Some(cfg) = cfg_parsed.as_ref() {
                debug!("Build URL parsing failed initially but cleaned; host differs, will use configured host '{}', path '{}'", cfg.host_str().unwrap_or(""), parsed.path());
                if parsed.scheme() == cfg.scheme() && parsed.host_str() == cfg.host_str() {
                    return Ok(parsed.join("api/json").context("Failed to append api/json to URL")?.into());
                }
            }

            // Hosts differ here too; re-use the parsed path with the configured
            // base so the monitoring uses the canonical configured host.
            let path = parsed.path();
            let mut safe_path = path.to_string();
            if !safe_path.ends_with('/') {
                safe_path.push('/');
            }

            let cfg = url::Url::parse(configured_base).context("Invalid configured Jenkins base URL")?;
            let joined = cfg.join(&safe_path).context("Failed to join configured base with build path")?;
            Ok(joined.join("api/json").context("Failed to append api/json to URL")?.into())
        }
    }
}

#[cfg(test)]
mod build_url_tests {
    use super::*;

    #[test]
    fn build_url_with_trailing_slash() {
        let raw = "https://jenkins.local/job/myjob/15/";
        let cfg = "https://jenkins.local/";
        let out = build_api_url_from_last_build(raw, cfg).expect("should build url");
        assert_eq!(out, "https://jenkins.local/job/myjob/15/api/json");
    }

    #[test]
    fn build_url_without_trailing_slash() {
        let raw = "https://jenkins.local/job/myjob/15";
        let cfg = "https://jenkins.local/";
        let out = build_api_url_from_last_build(raw, cfg).expect("should build url");
        assert_eq!(out, "https://jenkins.local/job/myjob/15/api/json");
    }

    #[test]
    fn build_url_with_spaces() {
        let raw = "https://jenkins.local/job/my folder/15";
        let cfg = "https://jenkins.local/";
        let out = build_api_url_from_last_build(raw, cfg).expect("should build url");
        assert_eq!(out, "https://jenkins.local/job/my%20folder/15/api/json");
    }

    #[test]
    fn uses_configured_host_when_raw_is_ip() {
        // Jenkins returned an IP-based last_build URL but configured base is the
        // canonical hostname — the monitor should use the configured host and
        // reuse the path.
        let raw = "http://192.168.0.196:8080/job/hourly-tests/1/";
        let cfg = "https://jenkins.local.starcher.dev/";
        let out = build_api_url_from_last_build(raw, cfg).expect("should build url");
        assert_eq!(out, "https://jenkins.local.starcher.dev/job/hourly-tests/1/api/json");
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
                // Use pretty debug formatting to include the error chain and contexts
                // so logs show the root cause (eg. URL parse error, HTTP status, etc.)
                error!("Error checking job '{}' : {:#}", job_config.name, e);
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

#[cfg(test)]
mod tests {
    use super::build_job_api_url;

    #[test]
    fn builds_top_level_job_url() {
        let base = "https://jenkins.example.com/";
        let job = "nightly-build";
        let got = build_job_api_url(base, job);
        assert_eq!(got, "https://jenkins.example.com/job/nightly-build/api/json");
    }

    #[test]
    fn builds_nested_job_url() {
        let base = "https://jenkins.example.com";
        let job = "folder/subfolder/nightly build"; // space to ensure encoding
        let got = build_job_api_url(base, job);
        assert_eq!(got, "https://jenkins.example.com/job/folder/job/subfolder/job/nightly%20build/api/json");
    }
}
