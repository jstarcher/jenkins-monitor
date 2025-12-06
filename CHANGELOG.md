# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
