# Jenkins Monitor 🔍

> *"I'm watching the watchers, Jerry!"* — Kramer

[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)](LICENSE)

> Ensure Jenkins actually runs jobs when you expect them to, and get alerted when they don't.

Jenkins Monitor is a lightweight monitoring tool designed to verify that your Jenkins jobs execute on their expected schedules. It proactively alerts you when jobs fail to run, helping you catch scheduling issues, configuration problems, or Jenkins infrastructure failures before they impact your development workflow.

## 🎯 Features

### Available in MVP
- ✅ **Schedule Monitoring**: Define expected job schedules using cron expressions
- ✅ **Missed Job Detection**: Automatically detect when jobs don't run as expected
- ✅ **Build Failure Alerts**: If a job's last build completed with a non-success status (e.g. FAILURE, UNSTABLE, ABORTED) the monitor will alert immediately
- ✅ **Email Alerts**: Send notifications via SMTP when jobs miss their schedule
- ✅ **Multiple Job Support**: Monitor multiple jobs across one Jenkins instance
- ✅ **Low Resource Usage**: Written in Rust for performance and reliability
- ✅ **TOML Configuration**: Simple configuration file format

### Planned Features
- 🔜 **Multi-Channel Alerts**: Slack, PagerDuty, webhooks, and other notification channels
- 🔜 **Multiple Jenkins Support**: Monitor jobs across multiple Jenkins instances
- 🔜 **Metrics Export**: Prometheus-compatible metrics for Grafana dashboards
- 🔜 **Docker Ready**: Easy deployment with Docker and Kubernetes
- 🔜 **Database Persistence**: Alert history and state tracking

## 🚀 Quick Start

**⚡ Want to get started fast?** See [QUICKSTART.md](QUICKSTART.md) for a 5-minute guide.

### Installation

```bash
# Build from source (requires Rust)
git clone https://github.com/jstarcher/jenkins-monitor.git
cd jenkins-monitor
cargo build --release

# The binary will be at target/release/jenkins-monitor
```

If you prefer a small convenience wrapper, there's a top-level `Makefile` with common targets. Examples:

```bash
# Build a release binary
make build

# Run the release binary
make run

# Run tests
make test

# Install the binary into $CARGO_HOME/bin
make install
```

Releases & CI
----------------

This repository includes GitHub Actions workflows:

- `.github/workflows/ci.yml` — runs on commits and PRs to `master` and executes tests and builds the release artifact.
- `.github/workflows/release.yml` — runs on tag pushes that match `v*` (for example `v0.1.0`). It builds release artifacts, creates a GitHub Release and attaches a binary tarball and a Debian package (if cargo-deb is enabled).

To publish a release from this repo you can tag & push:

```bash
git tag v0.1.0
git push origin v0.1.0
```

The release workflow will run and attach the compiled artifacts.

Installing as a systemd service
-------------------------------

If you prefer to run this as a system service on Debian, you can use `make install-service` on the target machine (requires sudo). This will:

- copy the binary to `/usr/local/bin/jenkins-monitor`
- install the `packaging/jenkins-monitor.service` systemd unit to `/etc/systemd/system`
- create a `jenkins-monitor` system user and directories under `/etc/jenkins-monitor` and `/var/lib/jenkins-monitor`
- enable and start the service via `systemctl`

You can also install the Debian package (`.deb`) produced by the release workflow, or build one locally with `cargo deb`:

```bash
# build a .deb locally (requires cargo-deb)
cargo deb
# install the produced deb
sudo dpkg -i target/debian/jenkins-monitor_*.deb
```


### Basic Configuration

Create a `config.toml` file:

```toml
[general]
log_level = "info"
check_interval_seconds = 60

[jenkins]
url = "https://jenkins.example.com"
username = "monitor-user"
password = "your-api-token-here"

[[job]]
name = "nightly-build"
# The `schedule` field is optional — if omitted the monitor will attempt to
# read the job's cron spec from Jenkins' `config.xml`. An explicit schedule in
# your `config.toml` will override the value found in Jenkins.
schedule = "0 0 2 * * *"  # Daily at 2 AM UTC (cron format with seconds)
alert_threshold_minutes = 90  # Alert if job hasn't run in 90 minutes
# Note: Jenkins often uses 5-field cron specs (no seconds) such as `0 0 * * *`.
# This monitor will automatically normalize 5-field cron expressions by
# prepending a leading seconds field of `0` so both forms are accepted.

[[job]]
name = "hourly-tests"
schedule = "0 0 * * * *"  # Every hour
alert_threshold_minutes = 75  # Alert if job hasn't run in 75 minutes

# Email alerts (optional)
[email]
smtp_host = "smtp.gmail.com"
smtp_port = 587
# Enable STARTTLS to upgrade the SMTP connection to TLS. Set to false to
# use an unencrypted connection (not recommended).
smtp_tls = true
from = "jenkins-monitor@example.com"
to = ["ops-team@example.com", "admin@example.com"]
username = "your-email@gmail.com"
password = "your-app-password"
```

### Run

```bash
# Run the monitor (config.toml must be in current directory)
./target/release/jenkins-monitor

# Or use cargo run
cargo run --release
```

## 📋 Use Cases

### Development Teams
- Ensure nightly builds run reliably
- Monitor critical CI/CD pipelines
- Get immediate notifications when scheduled jobs fail to execute

### DevOps/SRE Teams
- Verify backup jobs run on schedule
- Monitor deployment pipelines across multiple environments
- Track job execution patterns and trends

### QA Teams
- Ensure automated test suites run as expected
- Monitor regression test schedules
- Verify integration test execution

## 🏗️ Project Status

**Current Phase**: MVP Complete ✅

The MVP is now functional with the following features:
- ✅ Jenkins API integration for job monitoring
- ✅ Cron-based schedule checking
- ✅ Email alerts via SMTP
- ✅ Configurable check intervals and thresholds
- ✅ Basic logging and error handling

**What works now:**
- Monitor multiple Jenkins jobs on a schedule
- Check if jobs run according to their expected cron schedule
- Send email alerts when jobs fail to run on time
- Basic Jenkins authentication support

**Coming soon:**
- Additional alert channels (Slack, PagerDuty, etc.)
- Database persistence for alert history
- Web dashboard
- Advanced monitoring features

See [ROADMAP.md](ROADMAP.md) for detailed development plans and future features.

## 📚 Documentation

- **[ROADMAP.md](ROADMAP.md)** - Project roadmap with planned features and milestones
- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Technical architecture and design decisions
- **[CONTRIBUTING.md](CONTRIBUTING.md)** - How to contribute to the project
- **[CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)** - Community guidelines

## 🤝 Contributing

We welcome contributions! Whether you're fixing bugs, adding features, improving documentation, or spreading the word, your help is appreciated.

**Getting Started:**
1. Read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines
2. Check out the [ROADMAP.md](ROADMAP.md) for planned features
3. Look for issues labeled `good first issue`
4. Join discussions in GitHub Issues

**Areas Where We Need Help:**
- Core Rust development
- Jenkins API integration
- Alert channel implementations
- Documentation and examples
- Testing and QA
- UI/UX design (for future web interface)

## 🛣️ Roadmap Highlights

### Phase 1: Foundation (Q1 2026)
- Core Jenkins API integration
- Basic job monitoring
- Console and email alerts
- Configuration system

### Phase 2: Alerting (Q2 2026)
- Slack, Teams, PagerDuty integration
- Alert deduplication
- Alert history

### Phase 3: Advanced Monitoring (Q3 2026)
- Anomaly detection
- Prometheus metrics
- Multi-Jenkins support
- Web dashboard

See [ROADMAP.md](ROADMAP.md) for complete details.

## 🔒 Security

Security is a top priority. Jenkins Monitor:
- Uses read-only Jenkins API access
- Supports secure credential storage
- Communicates over HTTPS
- Never logs sensitive information

Found a security issue? Please report it privately to the maintainers.

## 📊 Why Rust?

We chose Rust for several key reasons:
- **Performance**: Low resource usage suitable for always-on monitoring
- **Reliability**: Memory safety prevents entire classes of bugs
- **Concurrency**: Safe async/await for monitoring multiple jobs
- **Deployment**: Single binary with no runtime dependencies

## 🔗 Related Projects

- [Jenkins](https://www.jenkins.io/) - The automation server this tool monitors
- [Prometheus](https://prometheus.io/) - Metrics and monitoring (future integration)
- [Grafana](https://grafana.com/) - Visualization (future dashboard templates)

## 📜 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 👥 Authors

- **Jordan Starcher** - *Initial work* - [@jstarcher](https://github.com/jstarcher)

See also the list of [contributors](https://github.com/jstarcher/jenkins-monitor/contributors) who participated in this project.

## 🙏 Acknowledgments

- The Jenkins community for building an amazing automation platform
- The Rust community for excellent tools and libraries
- All contributors who help make this project better

## 💬 Community

- **Issues**: [GitHub Issues](https://github.com/jstarcher/jenkins-monitor/issues)
- **Discussions**: [GitHub Discussions](https://github.com/jstarcher/jenkins-monitor/discussions)

---

**Status**: 🚀 MVP Ready - Basic monitoring with email alerts is functional

**Last Updated**: 2025-12-06
