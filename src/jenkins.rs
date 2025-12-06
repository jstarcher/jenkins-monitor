use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::time::Duration;

use crate::config::JenkinsConfig;

pub struct JenkinsClient {
    client: reqwest::Client,
    base_url: String,
    username: Option<String>,
    api_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct JobInfo {
    #[serde(rename = "lastBuild")]
    last_build: Option<BuildInfo>,
}

#[derive(Debug, Deserialize)]
struct BuildInfo {
    number: u64,
    timestamp: i64,
    result: Option<String>,
}

#[derive(Debug, Clone)]
pub struct LastBuildInfo {
    pub number: u64,
    pub timestamp: DateTime<Utc>,
    pub result: Option<String>,
}

impl JenkinsClient {
    pub fn new(config: &JenkinsConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .context("Failed to create HTTP client")?;
        
        Ok(Self {
            client,
            base_url: config.url.trim_end_matches('/').to_string(),
            username: config.username.clone(),
            api_token: config.api_token.clone(),
        })
    }
    
    pub async fn get_last_build(&self, job_name: &str) -> Result<Option<LastBuildInfo>> {
        let url = format!("{}/job/{}/api/json", self.base_url, job_name);
        
        log::debug!("Fetching job info from: {}", url);
        
        let mut request = self.client.get(&url);
        
        // Add basic auth if credentials are provided
        if let (Some(username), Some(token)) = (&self.username, &self.api_token) {
            request = request.basic_auth(username, Some(token));
        }
        
        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to fetch job info for '{}'", job_name))?;
        
        if !response.status().is_success() {
            anyhow::bail!(
                "Jenkins API returned error status {} for job '{}'",
                response.status(),
                job_name
            );
        }
        
        let job_info: JobInfo = response
            .json()
            .await
            .with_context(|| format!("Failed to parse JSON response for job '{}'", job_name))?;
        
        Ok(job_info.last_build.map(|build| {
            let timestamp = DateTime::from_timestamp_millis(build.timestamp)
                .unwrap_or_else(|| Utc::now());
            
            LastBuildInfo {
                number: build.number,
                timestamp,
                result: build.result,
            }
        }))
    }
    
    pub async fn test_connection(&self) -> Result<()> {
        let url = format!("{}/api/json", self.base_url);
        
        let mut request = self.client.get(&url);
        
        if let (Some(username), Some(token)) = (&self.username, &self.api_token) {
            request = request.basic_auth(username, Some(token));
        }
        
        let response = request
            .send()
            .await
            .context("Failed to connect to Jenkins")?;
        
        if !response.status().is_success() {
            anyhow::bail!(
                "Jenkins API returned error status: {}",
                response.status()
            );
        }
        
        Ok(())
    }
}
