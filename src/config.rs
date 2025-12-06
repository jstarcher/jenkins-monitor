use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub jenkins: JenkinsConfig,
    pub jobs: Vec<JobConfig>,
    pub alerts: AlertConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JenkinsConfig {
    pub url: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub api_token: Option<String>,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    30
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct JobConfig {
    pub name: String,
    pub expected_schedule: String, // Cron expression
    #[serde(default = "default_alert_threshold")]
    pub alert_threshold_mins: u64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_alert_threshold() -> u64 {
    60
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AlertConfig {
    #[serde(default)]
    pub email: Option<EmailConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EmailConfig {
    pub smtp_host: String,
    #[serde(default = "default_smtp_port")]
    pub smtp_port: u16,
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub password: Option<String>,
}

fn default_smtp_port() -> u16 {
    587
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config file: {:?}", path.as_ref()))?;
        
        let config: Config = toml::from_str(&content)
            .with_context(|| "Failed to parse config file")?;
        
        // Validate configuration
        config.validate()?;
        
        Ok(config)
    }
    
    fn validate(&self) -> Result<()> {
        if self.jenkins.url.is_empty() {
            anyhow::bail!("Jenkins URL cannot be empty");
        }
        
        if self.jobs.is_empty() {
            anyhow::bail!("At least one job must be configured");
        }
        
        for job in &self.jobs {
            if job.name.is_empty() {
                anyhow::bail!("Job name cannot be empty");
            }
            
            // Validate cron expression
            cron::Schedule::from_str(&job.expected_schedule)
                .with_context(|| format!("Invalid cron expression for job '{}': {}", job.name, job.expected_schedule))?;
        }
        
        Ok(())
    }
}
