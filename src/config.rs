use anyhow::{bail, Context, Result};
use chrono_tz::Tz;
use serde::Deserialize;
use std::fmt;
use std::fs;
use std::path::Path;

/// Environment variables that override secrets in the config file, so the
/// file itself can stay free of credentials.
pub const ENV_JENKINS_USERNAME: &str = "JENKINS_MONITOR_JENKINS_USERNAME";
pub const ENV_JENKINS_PASSWORD: &str = "JENKINS_MONITOR_JENKINS_PASSWORD";
pub const ENV_SMTP_USERNAME: &str = "JENKINS_MONITOR_SMTP_USERNAME";
pub const ENV_SMTP_PASSWORD: &str = "JENKINS_MONITOR_SMTP_PASSWORD";

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub general: ConfigGeneral,
    pub jenkins: ConfigJenkins,
    pub job: Vec<ConfigJob>,
    pub email: Option<ConfigEmail>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConfigGeneral {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_check_interval")]
    pub check_interval_seconds: u64,
    /// Alert when the monitor itself can't check a job (Jenkins down, bad
    /// credentials, unparseable schedule...).
    #[serde(default = "default_true")]
    pub alert_on_check_error: bool,
    /// How many checks in a row must fail before a check-error alert is
    /// sent, so one network blip doesn't page anyone.
    #[serde(default = "default_error_alert_after")]
    pub error_alert_after: u32,
    /// Send the same problem again after this many minutes while it is still
    /// unresolved. 0 means alert once per problem.
    #[serde(default)]
    pub realert_minutes: u64,
    /// Send a "recovered" email when a job goes back to healthy.
    #[serde(default = "default_true")]
    pub send_recovery: bool,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConfigJenkins {
    pub url: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// Time zone the Jenkins controller evaluates cron specs in (its JVM
    /// default), for example "America/New_York". Defaults to UTC. A `TZ=`
    /// line inside a job's spec still wins.
    #[serde(default = "default_timezone")]
    pub timezone: String,
}

impl fmt::Debug for ConfigJenkins {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigJenkins")
            .field("url", &self.url)
            .field("username", &self.username)
            .field("password", &redact(&self.password))
            .field("timezone", &self.timezone)
            .finish()
    }
}

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConfigJob {
    pub name: String,
    /// Optional: if not provided, the job's timer trigger is read from its
    /// Jenkins `config.xml` on every check.
    pub schedule: Option<String>,
    /// How long after a scheduled time a build may start before the run
    /// counts as missed.
    #[serde(default = "default_alert_threshold")]
    pub alert_threshold_minutes: i64,
    /// Per-job override of `general.alert_on_check_error`.
    pub alert_on_error: Option<bool>,
    /// Alert when the last completed build didn't succeed. Defaults to true.
    #[serde(default = "default_true")]
    pub alert_on_failure: bool,
}

#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SmtpSecurity {
    /// Plain connection upgraded with STARTTLS (usually port 587).
    Starttls,
    /// TLS from the first byte (usually port 465).
    Tls,
    /// No encryption. Only for a local relay.
    None,
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConfigEmail {
    pub smtp_host: String,
    pub smtp_port: u16,
    /// Older setting: true = STARTTLS, false = no encryption. Ignored when
    /// `smtp_security` is set.
    #[serde(default = "default_true")]
    pub smtp_tls: bool,
    pub smtp_security: Option<SmtpSecurity>,
    pub from: String,
    pub to: Vec<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl ConfigEmail {
    pub fn security(&self) -> SmtpSecurity {
        self.smtp_security.unwrap_or(if self.smtp_tls {
            SmtpSecurity::Starttls
        } else {
            SmtpSecurity::None
        })
    }
}

impl fmt::Debug for ConfigEmail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigEmail")
            .field("smtp_host", &self.smtp_host)
            .field("smtp_port", &self.smtp_port)
            .field("security", &self.security())
            .field("from", &self.from)
            .field("to", &self.to)
            .field("username", &self.username)
            .field("password", &self.password.as_deref().map(redact))
            .finish()
    }
}

fn redact(s: &str) -> &'static str {
    if s.is_empty() {
        "<empty>"
    } else {
        "<redacted>"
    }
}

fn default_log_level() -> String {
    "info".to_string()
}
fn default_check_interval() -> u64 {
    60
}
fn default_true() -> bool {
    true
}
fn default_error_alert_after() -> u32 {
    3
}
fn default_alert_threshold() -> i64 {
    60
}
fn default_timezone() -> String {
    "UTC".to_string()
}

impl Config {
    /// Read, apply environment overrides, and validate.
    pub fn load(path: &Path) -> Result<Config> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("cannot read config file {}", path.display()))?;
        let mut cfg = Config::from_toml(&text)
            .with_context(|| format!("invalid config file {}", path.display()))?;
        cfg.apply_env(|k| std::env::var(k).ok());
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn from_toml(text: &str) -> Result<Config> {
        Ok(toml::from_str(text)?)
    }

    pub fn apply_env(&mut self, get: impl Fn(&str) -> Option<String>) {
        if let Some(v) = get(ENV_JENKINS_USERNAME) {
            self.jenkins.username = v;
        }
        if let Some(v) = get(ENV_JENKINS_PASSWORD) {
            self.jenkins.password = v;
        }
        if let Some(email) = self.email.as_mut() {
            if let Some(v) = get(ENV_SMTP_USERNAME) {
                email.username = Some(v);
            }
            if let Some(v) = get(ENV_SMTP_PASSWORD) {
                email.password = Some(v);
            }
        }
    }

    pub fn timezone(&self) -> Result<Tz> {
        self.jenkins
            .timezone
            .parse::<Tz>()
            .map_err(|e| anyhow::anyhow!("jenkins.timezone '{}': {}", self.jenkins.timezone, e))
    }

    pub fn validate(&self) -> Result<()> {
        let url = &self.jenkins.url;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            bail!(
                "jenkins.url must start with http:// or https://, got '{}'",
                url
            );
        }
        self.timezone()?;
        if self.general.check_interval_seconds == 0 {
            bail!("general.check_interval_seconds must be at least 1");
        }
        if self.job.is_empty() {
            bail!("no [[job]] entries configured");
        }
        let mut seen = std::collections::HashSet::new();
        for job in &self.job {
            if job.name.trim().is_empty() {
                bail!("a [[job]] entry has an empty name");
            }
            if !seen.insert(job.name.as_str()) {
                bail!("job '{}' is listed more than once", job.name);
            }
            if job.alert_threshold_minutes < 0 {
                bail!(
                    "job '{}': alert_threshold_minutes can't be negative",
                    job.name
                );
            }
        }
        if let Some(email) = &self.email {
            if email.to.is_empty() {
                bail!("email.to needs at least one address");
            }
            if email.username.is_some() != email.password.is_some() {
                bail!("email.username and email.password must be set together");
            }
        }
        Ok(())
    }
}

/// Log a warning when the config file can be read by any local user, since
/// it usually holds credentials. Call after logging is set up.
#[cfg(unix)]
pub fn warn_if_readable_by_others(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = fs::metadata(path) {
        if meta.permissions().mode() & 0o004 != 0 {
            log::warn!(
                "{} is readable by every user on this machine; it may hold credentials (chmod 640 or 600)",
                path.display()
            );
        }
    }
}

#[cfg(not(unix))]
pub fn warn_if_readable_by_others(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
        [general]
        log_level = "info"

        [jenkins]
        url = "https://jenkins.local"
        username = "u"
        password = "p"

        [[job]]
        name = "j1"
        schedule = "0 0 * * * *"
    "#;

    fn with_email(extra: &str) -> String {
        format!(
            "{}\n[email]\nsmtp_host = \"smtp.demo\"\nsmtp_port = 587\nfrom = \"a@b\"\nto = [\"a@b\"]\n{}",
            BASE, extra
        )
    }

    #[test]
    fn smtp_tls_defaults_to_starttls() {
        let c = Config::from_toml(&with_email("")).unwrap();
        assert_eq!(c.email.unwrap().security(), SmtpSecurity::Starttls);
    }

    #[test]
    fn smtp_tls_false_means_plain() {
        let c = Config::from_toml(&with_email("smtp_tls = false")).unwrap();
        assert_eq!(c.email.unwrap().security(), SmtpSecurity::None);
    }

    #[test]
    fn smtp_security_wins() {
        let c =
            Config::from_toml(&with_email("smtp_tls = false\nsmtp_security = \"tls\"")).unwrap();
        assert_eq!(c.email.unwrap().security(), SmtpSecurity::Tls);
    }

    #[test]
    fn defaults() {
        let c = Config::from_toml(BASE).unwrap();
        assert_eq!(c.general.check_interval_seconds, 60);
        assert!(c.general.alert_on_check_error);
        assert_eq!(c.general.error_alert_after, 3);
        assert_eq!(c.general.realert_minutes, 0);
        assert!(c.general.send_recovery);
        assert_eq!(c.jenkins.timezone, "UTC");
        assert_eq!(c.job[0].alert_threshold_minutes, 60);
        assert!(c.job[0].alert_on_failure);
        c.validate().unwrap();
    }

    #[test]
    fn env_overrides_secrets() {
        let mut c = Config::from_toml(&with_email("")).unwrap();
        c.apply_env(|k| match k {
            ENV_JENKINS_PASSWORD => Some("from-env".into()),
            ENV_SMTP_USERNAME => Some("mailer".into()),
            ENV_SMTP_PASSWORD => Some("smtp-secret".into()),
            _ => None,
        });
        assert_eq!(c.jenkins.username, "u");
        assert_eq!(c.jenkins.password, "from-env");
        let e = c.email.unwrap();
        assert_eq!(e.username.as_deref(), Some("mailer"));
        assert_eq!(e.password.as_deref(), Some("smtp-secret"));
    }

    #[test]
    fn debug_output_hides_passwords() {
        let mut c =
            Config::from_toml(&with_email("username = \"x\"\npassword = \"hunter2\"")).unwrap();
        c.jenkins.password = "jenkins-token".into();
        let dbg = format!("{:?}", c);
        assert!(!dbg.contains("hunter2"));
        assert!(!dbg.contains("jenkins-token"));
        assert!(dbg.contains("<redacted>"));
    }

    #[test]
    fn typos_are_rejected() {
        let bad = BASE.replace("schedule =", "schedul =");
        assert!(Config::from_toml(&bad).is_err());
    }

    #[test]
    fn validation_catches_mistakes() {
        let dup = format!("{}\n[[job]]\nname = \"j1\"\n", BASE);
        assert!(Config::from_toml(&dup).unwrap().validate().is_err());

        let bad_url = BASE.replace("https://jenkins.local", "jenkins.local");
        assert!(Config::from_toml(&bad_url).unwrap().validate().is_err());

        let bad_tz = BASE.replace(
            "password = \"p\"",
            "password = \"p\"\ntimezone = \"Nowhere/Land\"",
        );
        assert!(Config::from_toml(&bad_tz).unwrap().validate().is_err());

        let half_creds = with_email("username = \"x\"");
        assert!(Config::from_toml(&half_creds).unwrap().validate().is_err());
    }
}
