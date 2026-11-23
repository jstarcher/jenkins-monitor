mod config;
mod jenkins;
mod monitor;
mod notify;
mod schedule;

use anyhow::{bail, Context, Result};
use chrono::Utc;
use config::Config;
use jenkins::JenkinsClient;
use monitor::Monitor;
use notify::{EmailNotifier, LogNotifier, Notifier};
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::Duration;

const USAGE: &str = "\
Usage: jenkins-monitor [OPTIONS]

Options:
  -c, --config <PATH>  Config file (default: $JENKINS_MONITOR_CONFIG or ./config.toml)
      --check          Validate the config, show each job's schedule, and exit
      --once           Run one check cycle and exit (for cron or a Kubernetes CronJob)
  -h, --help           Show this help
  -V, --version        Show the version

Secrets can come from the environment instead of the file:
  JENKINS_MONITOR_JENKINS_USERNAME, JENKINS_MONITOR_JENKINS_PASSWORD,
  JENKINS_MONITOR_SMTP_USERNAME, JENKINS_MONITOR_SMTP_PASSWORD
";

#[derive(Debug, PartialEq)]
struct Args {
    config: PathBuf,
    check: bool,
    once: bool,
}

enum Parsed {
    Run(Args),
    Exit(String),
}

fn parse_args(
    args: impl IntoIterator<Item = String>,
    env_config: Option<String>,
) -> Result<Parsed> {
    let mut config = env_config.map(PathBuf::from);
    let mut check = false;
    let mut once = false;
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-c" | "--config" => {
                config = Some(PathBuf::from(it.next().context("--config needs a path")?));
            }
            s if s.starts_with("--config=") => {
                config = Some(PathBuf::from(&s["--config=".len()..]))
            }
            "--check" => check = true,
            "--once" => once = true,
            "-h" | "--help" => return Ok(Parsed::Exit(USAGE.to_string())),
            "-V" | "--version" => {
                return Ok(Parsed::Exit(format!(
                    "jenkins-monitor {}",
                    env!("CARGO_PKG_VERSION")
                )))
            }
            other => bail!("unknown argument '{}'\n\n{}", other, USAGE),
        }
    }
    Ok(Parsed::Run(Args {
        config: config.unwrap_or_else(|| PathBuf::from("config.toml")),
        check,
        once,
    }))
}

fn main() -> ExitCode {
    let args = match parse_args(
        std::env::args().skip(1),
        std::env::var("JENKINS_MONITOR_CONFIG").ok(),
    ) {
        Ok(Parsed::Run(a)) => a,
        Ok(Parsed::Exit(text)) => {
            println!("{}", text);
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {:#}", e);
            return ExitCode::from(2);
        }
    };

    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {:#}", e);
            ExitCode::FAILURE
        }
    }
}

fn run(args: Args) -> Result<()> {
    let cfg = Config::load(&args.config)?;

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(&cfg.general.log_level),
    )
    .init();
    config::warn_if_readable_by_others(&args.config);

    let jenkins = JenkinsClient::new(
        &cfg.jenkins.url,
        &cfg.jenkins.username,
        &cfg.jenkins.password,
    )?;
    let notifier: Box<dyn Notifier> = match &cfg.email {
        Some(e) => Box::new(EmailNotifier::new(e)?),
        None => Box::new(LogNotifier),
    };

    let interval = Duration::from_secs(cfg.general.check_interval_seconds);
    let jobs = cfg.job.clone();
    let banner = format!(
        "Watching {} jobs on {} (cron time zone {}), checking every {}s",
        cfg.job.len(),
        cfg.jenkins.url,
        cfg.jenkins.timezone,
        cfg.general.check_interval_seconds
    );
    let email_configured = cfg.email.is_some();
    let mut monitor = Monitor::new(cfg, jenkins, notifier)?;

    if args.check {
        let mut failed = false;
        for job in &jobs {
            match monitor.resolve_schedule(job) {
                Ok(s) => println!("ok    {}: {}", job.name, s.source().replace('\n', " | ")),
                Err(e) => {
                    failed = true;
                    println!("error {}: {:#}", job.name, e);
                }
            }
        }
        if failed {
            bail!("some jobs have no usable schedule");
        }
        return Ok(());
    }

    log::info!("Jenkins Monitor {} starting", env!("CARGO_PKG_VERSION"));
    log::info!("{}", banner);
    if !email_configured {
        log::warn!("No [email] section: alerts will only be logged");
    }

    loop {
        monitor.run_cycle(Utc::now());
        if args.once {
            return Ok(());
        }
        thread::sleep(interval);
    }
}

impl Notifier for Box<dyn Notifier> {
    fn notify(&self, subject: &str, body: &str) -> Result<()> {
        (**self).notify(subject, body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_args(v: &[&str], env: Option<&str>) -> Result<Parsed> {
        parse_args(v.iter().map(|s| s.to_string()), env.map(String::from))
    }

    #[test]
    fn default_config_path() {
        match run_args(&[], None).unwrap() {
            Parsed::Run(a) => assert_eq!(
                a,
                Args {
                    config: "config.toml".into(),
                    check: false,
                    once: false
                }
            ),
            _ => panic!(),
        }
    }

    #[test]
    fn env_then_flag_precedence() {
        match run_args(&[], Some("/etc/jm.toml")).unwrap() {
            Parsed::Run(a) => assert_eq!(a.config, PathBuf::from("/etc/jm.toml")),
            _ => panic!(),
        }
        match run_args(&["--config", "x.toml", "--once"], Some("/etc/jm.toml")).unwrap() {
            Parsed::Run(a) => {
                assert_eq!(a.config, PathBuf::from("x.toml"));
                assert!(a.once);
            }
            _ => panic!(),
        }
        match run_args(&["--config=y.toml", "--check"], None).unwrap() {
            Parsed::Run(a) => {
                assert_eq!(a.config, PathBuf::from("y.toml"));
                assert!(a.check);
            }
            _ => panic!(),
        }
    }

    #[test]
    fn bad_args() {
        assert!(run_args(&["--nope"], None).is_err());
        assert!(run_args(&["--config"], None).is_err());
        assert!(matches!(
            run_args(&["--help"], None).unwrap(),
            Parsed::Exit(_)
        ));
    }
}
