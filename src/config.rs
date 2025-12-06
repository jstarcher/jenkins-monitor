use serde::Deserialize;
use std::fs;

#[derive(Deserialize, Debug, Clone)]
pub struct Config {
    pub general: ConfigGeneral,
    pub jenkins: ConfigJenkins,
    pub job: Vec<ConfigJob>,
    pub email: Option<ConfigEmail>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ConfigGeneral {
    pub log_level: String,
    #[serde(default = "default_check_interval")]
    pub check_interval_seconds: u64,
}

fn default_check_interval() -> u64 {
    60
}

#[derive(Deserialize, Debug, Clone)]
pub struct ConfigJenkins {
    pub url: String,
    pub username: String,
    pub password: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ConfigJob {
    pub name: String,
    pub schedule: String,
    #[serde(default = "default_alert_threshold")]
    pub alert_threshold_minutes: i64,
}

fn default_alert_threshold() -> i64 {
    60
}

#[derive(Deserialize, Debug, Clone)]
pub struct ConfigEmail {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub from: String,
    pub to: Vec<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

pub struct ConfigReader;

impl ConfigReader {
    pub fn make() -> Config {
        let conf = fs::read_to_string("config.toml").expect("cannot find config file");
        toml::from_str(&conf).expect("syntax error in config file")
    }
}
