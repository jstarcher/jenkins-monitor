# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - Unreleased

### Fixed
- Missed runs were flagged as soon as a scheduled time arrived, before
  Jenkins could start the build. A run now counts as missed only once its
  scheduled time plus `alert_threshold_minutes` has passed.
- Jobs whose interval was more than twice their threshold (for example a
  daily job with a 90 minute threshold) produced a check error, and an alert
  email, on every cycle for most of the day.
- Jenkins day-of-week numbers were off by one: `1` meant Sunday, not Monday,
  and `0` was rejected.
- Every unhealthy job sent an email on every check interval.
- An SCM polling spec in `config.xml` was mistaken for the build schedule.
- Release workflow could not upload the .deb and hid cargo-deb failures.

### Added
- Jenkins schedule syntax: `H`, `H/n`, `H(a-b)`, `@daily` and other aliases,
  multi-line specs, comments and `TZ=` lines.
- `jenkins.timezone` for the Jenkins controller's time zone.
- One alert per problem, `general.realert_minutes`, recovery emails
  (`general.send_recovery`), and `general.error_alert_after` so a single
  network blip doesn't alert.
- `--config`, `--check`, `--once`, `--version`; `JENKINS_MONITOR_CONFIG`.
- `JENKINS_MONITOR_*` environment variables for Jenkins and SMTP credentials.
- `email.smtp_security` (`starttls`, `tls`, `none`), adding implicit TLS.
- Per-job `alert_on_failure`.

### Changed
- Failure alerts look at the last completed build, so a running build no
  longer hides a failure.
- Unknown config keys, duplicate jobs and invalid values now fail at startup.
- One Jenkins API request per job instead of two.
- Dependencies updated; TLS uses rustls.
- systemd unit passes `--config`, reads `/etc/jenkins-monitor/env`, no longer
  forces `RUST_LOG=info`, and adds sandboxing.

## [0.1.0] - 2025-12-06

### Added
- Initial MVP release with core monitoring functionality
- Jenkins API integration for job monitoring
- Cron-based schedule checking with seconds precision
- Email alert system via SMTP
- TOML configuration file support
- Configurable check intervals and alert thresholds
- Basic logging with configurable log levels
- Support for monitoring multiple jobs
- Example configuration file (config.example.toml)
- Comprehensive usage documentation (USAGE.md)

### Features
- Monitor Jenkins jobs and verify they run according to schedule
- Send email alerts when jobs miss their expected execution time
- Support for standard cron expressions (with seconds)
- Flexible alert thresholds per job
- Jenkins authentication via username/password or API token
- SMTP authentication support for email alerts
- Multiple email recipients per alert

### Technical
- Built with Rust 2021 edition
- Uses modern dependencies (reqwest, lettre, cron)
- Minimal resource usage
- Single binary deployment
- Cross-platform support (Linux, macOS, Windows)

[0.1.0]: https://github.com/jstarcher/jenkins-monitor/releases/tag/v0.1.0
