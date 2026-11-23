//! Minimal Jenkins REST client: one request for a job's build state, one for
//! its `config.xml`.

use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, TimeZone, Utc};
use reqwest::blocking::{Client, Response};
use serde::Deserialize;
use std::thread;
use std::time::Duration;

/// Fields we ask Jenkins for. `lastBuild` may still be running; the last
/// *completed* build is what tells us whether the job is failing.
const JOB_TREE: &str = "lastBuild[number,timestamp,result,url],\
                        lastCompletedBuild[number,timestamp,result,url]";

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Build {
    pub number: i64,
    /// Start time, milliseconds since the epoch.
    pub timestamp: i64,
    /// `None` while the build is running.
    pub result: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

impl Build {
    pub fn started_at(&self) -> Result<DateTime<Utc>> {
        Utc.timestamp_millis_opt(self.timestamp)
            .single()
            .ok_or_else(|| anyhow!("invalid build timestamp {}", self.timestamp))
    }

    pub fn succeeded(&self) -> bool {
        self.result.as_deref() == Some("SUCCESS")
    }
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct JobBuilds {
    #[serde(rename = "lastBuild")]
    pub last_build: Option<Build>,
    #[serde(rename = "lastCompletedBuild")]
    pub last_completed_build: Option<Build>,
}

pub trait JenkinsApi {
    fn job_builds(&self, job_name: &str) -> Result<JobBuilds>;
    fn job_config_xml(&self, job_name: &str) -> Result<String>;
}

pub struct JenkinsClient {
    base_url: String,
    username: String,
    password: String,
    http: Client,
    max_attempts: usize,
    base_delay: Duration,
}

impl JenkinsClient {
    pub fn new(base_url: &str, username: &str, password: &str) -> Result<Self> {
        let http = Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("jenkins-monitor/", env!("CARGO_PKG_VERSION")))
            .build()
            .context("failed to build HTTP client")?;
        Ok(JenkinsClient {
            base_url: base_url.trim_end_matches('/').to_string(),
            username: username.to_string(),
            password: password.to_string(),
            http,
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
        })
    }

    #[cfg(test)]
    fn with_retry(mut self, max_attempts: usize, base_delay: Duration) -> Self {
        self.max_attempts = max_attempts;
        self.base_delay = base_delay;
        self
    }

    /// GET with retries and exponential backoff on network errors, 5xx and
    /// 429. Non-success statuses after the last attempt become errors.
    fn get(&self, url: &str) -> Result<Response> {
        let mut delay = self.base_delay;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut req = self.http.get(url);
            if !self.username.is_empty() || !self.password.is_empty() {
                req = req.basic_auth(&self.username, Some(&self.password));
            }
            let retryable = match req.send() {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp);
                    }
                    if !(status.is_server_error() || status.as_u16() == 429)
                        || attempt >= self.max_attempts
                    {
                        bail!("Jenkins returned HTTP {} for {}", status, url);
                    }
                    format!("HTTP {}", status)
                }
                Err(e) => {
                    if attempt >= self.max_attempts {
                        return Err(anyhow!(e).context(format!(
                            "request to {} failed after {} attempts",
                            url, attempt
                        )));
                    }
                    e.to_string()
                }
            };
            log::debug!(
                "GET {} failed (attempt {}): {}; retrying in {:?}",
                url,
                attempt,
                retryable,
                delay
            );
            thread::sleep(delay);
            delay = delay.saturating_mul(2);
        }
    }
}

impl JenkinsApi for JenkinsClient {
    fn job_builds(&self, job_name: &str) -> Result<JobBuilds> {
        let url = format!(
            "{}/api/json?tree={}",
            job_path(&self.base_url, job_name),
            urlencoding::encode(JOB_TREE)
        );
        self.get(&url)?
            .json()
            .with_context(|| format!("unexpected JSON from {}", url))
    }

    fn job_config_xml(&self, job_name: &str) -> Result<String> {
        let url = format!("{}/config.xml", job_path(&self.base_url, job_name));
        self.get(&url)?
            .text()
            .with_context(|| format!("failed to read {}", url))
    }
}

/// `folder/sub/job` -> `{base}/job/folder/job/sub/job/job`, each part encoded.
pub fn job_path(base_url: &str, job_name: &str) -> String {
    let mut url = base_url.trim_end_matches('/').to_string();
    for part in job_name.split('/').filter(|p| !p.is_empty()) {
        url.push_str("/job/");
        url.push_str(&urlencoding::encode(part));
    }
    url
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_job_path() {
        assert_eq!(
            job_path("https://jenkins.example.com/", "nightly-build"),
            "https://jenkins.example.com/job/nightly-build"
        );
    }

    #[test]
    fn nested_job_path_is_encoded() {
        assert_eq!(
            job_path("https://jenkins.example.com/ci", "folder/sub/nightly build"),
            "https://jenkins.example.com/ci/job/folder/job/sub/job/nightly%20build"
        );
    }

    #[test]
    fn fetches_builds_with_tree_query_and_auth() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/job/app/job/nightly/api/json")
            .match_query(mockito::Matcher::UrlEncoded("tree".into(), JOB_TREE.into()))
            .match_header("authorization", mockito::Matcher::Regex("^Basic ".into()))
            .with_body(
                r#"{"_class":"x","lastBuild":{"number":8,"timestamp":1700000000000,"result":null,"url":"u"},
                    "lastCompletedBuild":{"number":7,"timestamp":1699990000000,"result":"FAILURE"}}"#,
            )
            .create();

        let c = JenkinsClient::new(&server.url(), "user", "token").unwrap();
        let b = c.job_builds("app/nightly").unwrap();
        m.assert();
        assert_eq!(b.last_build.as_ref().unwrap().number, 8);
        assert_eq!(b.last_build.as_ref().unwrap().result, None);
        assert_eq!(
            b.last_completed_build.unwrap().result.as_deref(),
            Some("FAILURE")
        );
    }

    #[test]
    fn never_built_job() {
        let mut server = mockito::Server::new();
        let _m = server
            .mock("GET", "/job/new/api/json")
            .match_query(mockito::Matcher::Any)
            .with_body(r#"{"lastBuild":null,"lastCompletedBuild":null}"#)
            .create();
        let c = JenkinsClient::new(&server.url(), "", "").unwrap();
        assert_eq!(c.job_builds("new").unwrap(), JobBuilds::default());
    }

    #[test]
    fn retries_server_errors_then_succeeds() {
        let mut server = mockito::Server::new();
        let fail = server
            .mock("GET", "/job/j/config.xml")
            .with_status(503)
            .expect(2)
            .create();
        let ok = server
            .mock("GET", "/job/j/config.xml")
            .with_body("<project/>")
            .expect(1)
            .create();
        let c = JenkinsClient::new(&server.url(), "", "")
            .unwrap()
            .with_retry(3, Duration::from_millis(1));
        assert_eq!(c.job_config_xml("j").unwrap(), "<project/>");
        fail.assert();
        ok.assert();
    }

    #[test]
    fn gives_up_after_max_attempts() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/job/j/config.xml")
            .with_status(500)
            .expect(3)
            .create();
        let c = JenkinsClient::new(&server.url(), "", "")
            .unwrap()
            .with_retry(3, Duration::from_millis(1));
        let err = c.job_config_xml("j").unwrap_err();
        m.assert();
        assert!(err.to_string().contains("500"), "{}", err);
    }

    #[test]
    fn client_errors_are_not_retried() {
        let mut server = mockito::Server::new();
        let m = server
            .mock("GET", "/job/j/config.xml")
            .with_status(401)
            .expect(1)
            .create();
        let c = JenkinsClient::new(&server.url(), "", "")
            .unwrap()
            .with_retry(3, Duration::from_millis(1));
        assert!(c.job_config_xml("j").is_err());
        m.assert();
    }
}
