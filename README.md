# Jenkins Monitor 🔍
[![CircleCI](https://circleci.com/gh/jstarcher/jenkins-monitor/tree/init.svg?style=svg&circle-token=148cadfd16b0ef17e70c115c368a7208681cf6e9)](https://circleci.com/gh/jstarcher/jenkins-monitor/tree/init)

Ensure Jenkins actually runs jobs when you expect it to and alert you if it did not

[![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg?style=for-the-badge)](LICENSE)

> Ensure Jenkins actually runs jobs when you expect them to, and get alerted when they don't.

Jenkins Monitor is a lightweight monitoring tool designed to verify that your Jenkins jobs execute on their expected schedules. It proactively alerts you when jobs fail to run, helping you catch scheduling issues, configuration problems, or Jenkins infrastructure failures before they impact your development workflow.

## 🎯 Features (Planned)

- **Schedule Monitoring**: Define expected job schedules using cron expressions
- **Missed Job Detection**: Automatically detect when jobs don't run as expected
- **Multi-Channel Alerts**: Send notifications via Email, Slack, PagerDuty, or custom webhooks
- **Multiple Jenkins Support**: Monitor jobs across multiple Jenkins instances
- **Low Resource Usage**: Written in Rust for performance and reliability
- **Flexible Configuration**: TOML/YAML/JSON configuration support
- **Metrics Export**: Prometheus-compatible metrics for Grafana dashboards
- **Docker Ready**: Easy deployment with Docker and Kubernetes

## 🚀 Quick Start

> **Note**: This project is currently in the planning phase. The quick start guide below reflects the intended usage once development is complete.

### Installation

```bash
# Install from source (requires Rust)
cargo install jenkins-monitor

# Or use Docker
docker pull jstarcher/jenkins-monitor:latest
```

### Basic Configuration

Create a `config.toml` file:

```toml
[jenkins]
url = "https://jenkins.example.com"
username = "monitor-user"
api_token = "your-api-token-here"

[[jobs]]
name = "nightly-build"
expected_schedule = "0 2 * * *"  # Daily at 2 AM UTC
alert_threshold = "1h"

[[jobs]]
name = "hourly-tests"
expected_schedule = "0 * * * *"  # Every hour
alert_threshold = "15m"

[alerts.slack]
webhook_url = "https://hooks.slack.com/services/YOUR/WEBHOOK/URL"
channel = "#jenkins-alerts"
```

### Run

```bash
# Run with config file
jenkins-monitor --config config.toml

# Run with Docker
docker run -v $(pwd)/config.toml:/config.toml jstarcher/jenkins-monitor --config /config.toml
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

**Current Phase**: Planning & Documentation

This project is currently in the initial planning stages. We are:
- ✅ Documenting architecture and design
- ✅ Creating project roadmap
- ✅ Defining core features and requirements
- 📝 Setting up contribution guidelines
- 🔜 Beginning Phase 1 implementation

See [ROADMAP.md](ROADMAP.md) for detailed development plans and timelines.

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

**Status**: 🚧 In Planning - Not yet ready for production use

**Last Updated**: 2025-12-06
