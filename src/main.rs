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
#[cfg(test)]
use std::sync::Mutex;

lazy_static! {
    static ref APP_CONF: Config = ConfigReader::make();
}

// Test-only capture for outgoing emails. Kept as a separate lazy_static so the
// main lazy_static expansion doesn't try to include a cfg-gated item during
// non-test builds (which breaks release builds).
#[cfg(test)]
lazy_static! {
    static ref TEST_SENT_EMAILS: Mutex<Vec<String>> = Mutex::new(vec![]);
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

// Return true when the build explicitly finished with a non-success result
// (e.g. FAILURE, UNSTABLE, ABORTED). A `None` result indicates the build is
// still running and should not be considered a failed build by this check.
fn is_build_failed(build: &BuildDetails) -> bool {
    match build.result.as_deref() {
        Some(r) => r != "SUCCESS",
        None => false,
    }
}

fn get_jenkins_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client")
}

/// Perform a blocking GET request with retries and exponential backoff.
///
/// - `max_attempts`: maximum number of attempts (>=1)
/// - `base_delay_ms`: initial delay in milliseconds between attempts; delay doubles each retry
fn http_get_with_retries(
    client: &Client,
    url: &str,
    username: &str,
    password: &str,
    max_attempts: usize,
    base_delay_ms: u64,
) -> Result<reqwest::blocking::Response> {
    if max_attempts == 0 {
        return Err(anyhow::anyhow!("max_attempts must be >= 1"));
    }

    let mut attempt = 0usize;
    let mut delay = Duration::from_millis(base_delay_ms);

    loop {
        attempt += 1;

        let res = client
            .get(url)
            .basic_auth(username, Some(password))
            .send();

        match res {
            Ok(resp) => {
                // If server error (5xx) we may want to retry
                if resp.status().is_server_error() && attempt < max_attempts {
                    debug!("Request to {} returned server error {} (attempt {}) - retrying after {:?}", url, resp.status(), attempt, delay);
                    thread::sleep(delay);
                    delay = delay.checked_mul(2u32).unwrap_or(delay);
                    continue;
                }

                return Ok(resp);
            }
            Err(e) => {
                if attempt >= max_attempts {
                    return Err(anyhow::anyhow!("request failed after {} attempts: {}", attempt, e));
                }

                debug!("Request to {} failed (attempt {}): {} - retrying after {:?}", url, attempt, e, delay);
                thread::sleep(delay);
                delay = delay.checked_mul(2u32).unwrap_or(delay);
                continue;
            }
        }
    }
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

// Build the URL pointing at a job's `config.xml` endpoint.
fn build_job_config_url(base_url: &str, job_name: &str) -> String {
    let mut url = base_url.trim_end_matches('/').to_string();

    for part in job_name.split('/') {
        let enc = urlencoding::encode(part);
        url.push_str(&format!("/job/{}", enc));
    }

    url.push_str("/config.xml");
    url
}

// Extract a cron schedule spec from a Jenkins `config.xml` body.
// Prefer the TimerTrigger-specific <spec> when present; otherwise fall back
// to the first <spec> element found.
fn extract_schedule_from_config_xml(body: &str) -> Option<String> {
    // Prefer <hudson.triggers.TimerTrigger> block
    if let Some(trigger_pos) = body.find("<hudson.triggers.TimerTrigger") {
        let sub = &body[trigger_pos..];
        if let Some(start) = sub.find("<spec>") {
            if let Some(end) = sub[start + 6..].find("</spec>") {
                let spec = &sub[start + 6..start + 6 + end];
                return Some(spec.trim().to_string());
            }
        }
    }

    // fallback: use the first <spec>...</spec> in the document
    if let Some(start) = body.find("<spec>") {
        if let Some(end) = body[start + 6..].find("</spec>") {
            let spec = &body[start + 6..start + 6 + end];
            return Some(spec.trim().to_string());
        }
    }

    None
}

fn fetch_job_schedule(job_name: &str) -> Result<String> {
    let client = get_jenkins_client();
    let url = build_job_config_url(&APP_CONF.jenkins.url, job_name);

    debug!("Fetching job config from: {}", url);

    let response = http_get_with_retries(
        &client,
        &url,
        &APP_CONF.jenkins.username,
        &APP_CONF.jenkins.password,
        3,
        500,
    )
    .context("Failed to fetch job config.xml from Jenkins")?;

    if !response.status().is_success() {
        return Err(anyhow::anyhow!(
            "Jenkins config.xml API returned error status: {}",
            response.status()
        ));
    }

    let body = response.text().context("Failed to read config.xml body")?;

    match extract_schedule_from_config_xml(&body) {
        Some(spec) => Ok(spec),
        None => Err(anyhow::anyhow!("No schedule <spec> found in job config.xml")),
    }
}

fn check_job(job_name: &str, schedule: &Schedule, threshold_minutes: i64) -> Result<bool> {
    let now = Utc::now();
    let client = get_jenkins_client();
    
    let job_url = build_job_api_url(&APP_CONF.jenkins.url, job_name);
    
    debug!("Fetching job info from: {}", job_url);
    
    // Try with retries/backoff in case of transient network failures
    let response = http_get_with_retries(
        &client,
        &job_url,
        &APP_CONF.jenkins.username,
        &APP_CONF.jenkins.password,
        3,
        500,
    )
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
        
        let build_response = http_get_with_retries(
            &client,
            &build_url,
            &APP_CONF.jenkins.username,
            &APP_CONF.jenkins.password,
            3,
            500,
        )
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
        // If the last build completed with a non-success result, treat that
        // as an immediate alert condition (the user requested simple behavior
        // to alert on failures). A `None` result means the build is still
        // running and is not considered a failed build here.
        if is_build_failed(&build) {
            warn!(
                "Job '{}' last build #{} finished with status {:?} — alerting",
                job_name,
                build.number,
                build.result.as_deref().unwrap_or("RUNNING")
            );

            return Ok(false);
        }
        
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

#[cfg(test)]
mod config_xml_tests {
        use super::extract_schedule_from_config_xml;

        #[test]
        fn extract_timer_trigger_spec() {
                let xml = r#"
                        <project>
                            <triggers>
                                <hudson.triggers.TimerTrigger>
                                    <spec> 0 0 * * * * </spec>
                                </hudson.triggers.TimerTrigger>
                            </triggers>
                        </project>
                "#;

                let got = extract_schedule_from_config_xml(xml).expect("should find spec");
                assert_eq!(got, "0 0 * * * *");
        }

        #[test]
        fn fallback_first_spec() {
                let xml = r#"
                        <project>
                            <scm>
                                <spec>H/15 * * * *</spec>
                            </scm>
                        </project>
                "#;

                let got = extract_schedule_from_config_xml(xml).expect("should find spec");
                assert_eq!(got, "H/15 * * * *");
        }
}

#[cfg(test)]
mod http_retry_tests {
    use super::*;
    use mockito::{mock, server_url};

    #[test]
    fn http_get_retries_and_succeeds() {
        let _m1 = mock("GET", "/retry")
            .with_status(500)
            .expect(2)
            .create();

        let _m2 = mock("GET", "/retry")
            .with_status(200)
            .with_body("{\"ok\":true}")
            .create();

        let client = get_jenkins_client();
        let url = format!("{}/retry", server_url());
        let resp = http_get_with_retries(&client, &url, "", "", 3, 1).expect("should succeed after retries");
        assert!(resp.status().is_success());
    }

    #[test]
    fn http_get_fails_after_max_attempts() {
        let _m = mock("GET", "/always500").with_status(500).expect(3).create();

        let client = get_jenkins_client();
        let url = format!("{}/always500", server_url());
        let res = http_get_with_retries(&client, &url, "", "", 3, 1);
        assert!(res.is_ok());
        // The returned response should be the last server error (500)
        let r = res.unwrap();
        assert_eq!(r.status().as_u16(), 500);
    }
}

#[cfg(test)]
mod alert_tests {
    use super::*;
    use anyhow::anyhow;

    #[test]
    fn format_check_error_alert_includes_job_and_error() {
        let err = anyhow!("network timeout").context("Failed to fetch job info from Jenkins");
        let msg = format_check_error_alert("nightly-build", &err);

        assert!(msg.contains("nightly-build"));
        assert!(msg.contains("network timeout"));
        assert!(msg.contains("Failed to fetch job info from Jenkins"));
    }
}

#[cfg(test)]
mod email_capture_tests {
    use super::*;

    #[test]
    fn send_email_alert_is_captured_in_tests() {
        // ensure the test capture vector is empty first
        TEST_SENT_EMAILS.lock().unwrap().clear();

        let msg = "Simulated failure";
        let res = send_email_alert("integration-tests", msg);
        assert!(res.is_ok());

        let emails = TEST_SENT_EMAILS.lock().unwrap();
        assert!(!emails.is_empty());
        assert!(emails[0].contains("integration-tests"));
        assert!(emails[0].contains(msg));
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
        // During unit/integration tests we capture outgoing email bodies into a
        // test-only in-memory vector so tests can assert that an email would
        // have been sent without needing a real SMTP server.
        #[cfg(test)]
        {
            let email_body = format!(
                "Jenkins Monitor Alert\n\nJob: {}\n\n{}\n\nTime: {}\nJenkins URL: {}\n",
                job_name,
                message,
                Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
                APP_CONF.jenkins.url
            );

            TEST_SENT_EMAILS.lock().unwrap().push(email_body);
            return Ok(());
        }

        #[cfg(not(test))]
        {
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
        
        // Allow TLS to be enabled/disabled by configuration. By default config
        // has TLS enabled (smtp_tls = true) and we'll attempt to use STARTTLS
        // to upgrade the connection. When disabled we create an unencrypted
        // builder (builder_dangerous) so callers can opt-out of TLS.
        let mut mailer_builder = if email_config.smtp_tls {
            SmtpTransport::starttls_relay(&email_config.smtp_host)?
        } else {
            SmtpTransport::builder_dangerous(&email_config.smtp_host)
        };

        mailer_builder = mailer_builder.port(email_config.smtp_port);
        
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
        }
    } else {
        warn!("Email not configured, skipping alert for job '{}'", job_name);
        Ok(())
    }
}

/// Format an alert message for a failed job check. We keep the message
/// concise since `send_email_alert` will wrap it into a larger email body.
fn format_check_error_alert(job_name: &str, error: &anyhow::Error) -> String {
    format!(
        "Failed to verify job '{}'. Error details:\n\n{}\n\nCheck the monitor logs for the full error chain.",
        job_name,
        format!("{:#}", error)
    )
}

fn monitor_jobs() {
    info!("Starting job monitoring cycle...");
    
    for job_config in &APP_CONF.job {
        // Determine the cron spec string. Prefer an explicit schedule from
        // the config.toml; otherwise try to fetch the job's schedule from
        // Jenkins' config.xml.
        let schedule_str = if let Some(s) = &job_config.schedule {
            s.clone()
        } else {
            match fetch_job_schedule(&job_config.name) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to determine schedule for job '{}': {:#}", job_config.name, e);
                    continue;
                }
            }
        };

        let schedule = match Schedule::from_str(&schedule_str) {
            Ok(s) => s,
            Err(e) => {
                error!("Invalid cron schedule '{}' for job '{}': {}", schedule_str, job_config.name, e);
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
                        schedule_str, job_config.alert_threshold_minutes
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

                // Convert the error into a user-facing alert message and try to
                // send an alert email (if configured). This ensures that
                // transient network failures like a timeout trigger an alert.
                let should_alert = job_config.alert_on_error
                    .unwrap_or(APP_CONF.general.alert_on_check_error);

                if should_alert {
                    let message = format_check_error_alert(&job_config.name, &e);
                    if let Err(send_err) = send_email_alert(&job_config.name, &message) {
                        error!("Failed to send alert for job '{}': {}", job_config.name, send_err);
                    }
                } else {
                    debug!("Alert-on-error disabled for job '{}' (global: {}) — not sending alert", job_config.name, APP_CONF.general.alert_on_check_error);
                }
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
    use super::{build_job_api_url, build_job_config_url};

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

    #[test]
    fn builds_top_level_config_url() {
        let base = "https://jenkins.example.com/";
        let job = "nightly-build";
        let got = build_job_config_url(base, job);
        assert_eq!(got, "https://jenkins.example.com/job/nightly-build/config.xml");
    }

    #[test]
    fn builds_nested_config_url() {
        let base = "https://jenkins.example.com";
        let job = "folder/subfolder/nightly build"; // space to ensure encoding
        let got = build_job_config_url(base, job);
        assert_eq!(got, "https://jenkins.example.com/job/folder/job/subfolder/job/nightly%20build/config.xml");
    }

    #[test]
    fn build_failed_detection() {
        let b = super::BuildDetails {
            number: 42,
            timestamp: 0,
            result: Some("FAILURE".to_string()),
            display_name: "#42".to_string(),
        };

        assert!(super::is_build_failed(&b));

        let s = super::BuildDetails {
            number: 43,
            timestamp: 0,
            result: Some("SUCCESS".to_string()),
            display_name: "#43".to_string(),
        };

        assert!(!super::is_build_failed(&s));

        let r = super::BuildDetails {
            number: 44,
            timestamp: 0,
            result: None,
            display_name: "#44".to_string(),
        };

        // running builds (None) are not treated as failures
        assert!(!super::is_build_failed(&r));
    }
}
