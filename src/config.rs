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
    #[serde(default = "default_alert_on_check_error")]
    pub alert_on_check_error: bool,
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
    // Optional per-job override of whether to alert when check_job() returns an error
    pub alert_on_error: Option<bool>,
}

fn default_alert_threshold() -> i64 {
    60
}

fn default_alert_on_check_error() -> bool {
    true
}

#[derive(Deserialize, Debug, Clone)]
pub struct ConfigEmail {
    pub smtp_host: String,
    pub smtp_port: u16,
    #[serde(default = "default_smtp_tls")]
    pub smtp_tls: bool,
    pub from: String,
    pub to: Vec<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

fn default_smtp_tls() -> bool {
    true
}

pub struct ConfigReader;

impl ConfigReader {
    pub fn make() -> Config {
        let conf = fs::read_to_string("config.toml").expect("cannot find config file");
        toml::from_str(&conf).expect("syntax error in config file")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_email_default_smtp_tls_true_when_missing() {
        let toml = r#"
            [general]
            log_level = "info"

            [jenkins]
            url = "https://jenkins.local"
            username = "u"
            password = "p"

            [[job]]
            name = "j1"
            schedule = "0 0 * * * *"

            [email]
            smtp_host = "smtp.demo"
            smtp_port = 587
            from = "a@b"
            to = ["a@b"]
        "#;

        let c: Config = toml::from_str(toml).expect("should parse");
        assert!(c.email.is_some());
        let e = c.email.unwrap();
        assert_eq!(e.smtp_tls, true, "smtp_tls defaults to true");
    }

    #[test]
    fn config_email_respects_smtp_tls_when_present() {
        let toml = r#"
            [general]
            log_level = "info"

            [jenkins]
            url = "https://jenkins.local"
            username = "u"
            password = "p"

            [[job]]
            name = "j1"
            schedule = "0 0 * * * *"

            [email]
            smtp_host = "smtp.demo"
            smtp_port = 587
            smtp_tls = false
            from = "a@b"
            to = ["a@b"]
        "#;

        let c: Config = toml::from_str(toml).expect("should parse");
        assert!(c.email.is_some());
        let e = c.email.unwrap();
        assert_eq!(e.smtp_tls, false, "smtp_tls set to false");
    }
}
