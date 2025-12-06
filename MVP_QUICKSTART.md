# Jenkins Monitor - MVP Quick Start Guide

This guide will help you get the MVP version of Jenkins Monitor up and running quickly.

## Prerequisites

- Rust 1.70 or later (install from https://rustup.rs/)
- Access to a Jenkins server
- Jenkins API credentials (username and API token)

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/jstarcher/jenkins-monitor.git
cd jenkins-monitor

# Build the project
cargo build --release

# The binary will be at target/release/jenkins-monitor
```

## Configuration

1. Copy the example configuration:
```bash
cp config.example.toml config.toml
```

2. Edit `config.toml` with your settings:

```toml
[jenkins]
url = "https://your-jenkins-server.com"
username = "your-username"
api_token = "your-api-token"

[[jobs]]
name = "your-job-name"
expected_schedule = "0 2 * * *"  # Daily at 2 AM
alert_threshold_mins = 60

[alerts.email]
smtp_host = "smtp.gmail.com"
smtp_port = 587
from = "jenkins-monitor@example.com"
to = ["your-email@example.com"]
username = "your-smtp-username"
password = "your-smtp-password"
```

### Getting Jenkins API Token

1. Log into your Jenkins server
2. Click on your username (top right)
3. Click "Configure"
4. Under "API Token", click "Add new Token"
5. Give it a name and click "Generate"
6. Copy the token to your config.toml

### Cron Expression Examples

- `0 * * * *` - Every hour
- `0 2 * * *` - Daily at 2 AM
- `0 */4 * * *` - Every 4 hours
- `0 0 * * 1` - Every Monday at midnight
- `*/15 * * * *` - Every 15 minutes

## Running

### Development Mode

```bash
# Run with default config.toml
RUST_LOG=info cargo run

# Run with custom config
RUST_LOG=info cargo run -- --config /path/to/config.toml
```

### Production Mode

```bash
# Build release version
cargo build --release

# Run the binary
RUST_LOG=info ./target/release/jenkins-monitor --config config.toml
```

### Running as a Service

#### systemd (Linux)

Create `/etc/systemd/system/jenkins-monitor.service`:

```ini
[Unit]
Description=Jenkins Monitor
After=network.target

[Service]
Type=simple
User=jenkins-monitor
WorkingDirectory=/opt/jenkins-monitor
ExecStart=/opt/jenkins-monitor/jenkins-monitor --config /opt/jenkins-monitor/config.toml
Environment="RUST_LOG=info"
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
```

Then:
```bash
sudo systemctl daemon-reload
sudo systemctl enable jenkins-monitor
sudo systemctl start jenkins-monitor
sudo systemctl status jenkins-monitor
```

## How It Works

1. **Monitoring Loop**: Jenkins Monitor checks your configured jobs every 60 seconds
2. **Schedule Checking**: For each job, it compares the last build time with the expected schedule
3. **Alert Threshold**: If a job hasn't run within the expected time + threshold, an alert is sent
4. **Email Alerts**: Alerts are sent via SMTP to the configured recipients
5. **Deduplication**: Alerts for the same job are not sent more than once per hour

## Troubleshooting

### Connection Issues

```bash
# Test connectivity with verbose logging
RUST_LOG=debug cargo run
```

Common issues:
- **Authentication failures**: Verify your Jenkins credentials
- **SSL errors**: Ensure your Jenkins URL uses HTTPS if required
- **Network timeouts**: Check firewall rules and network connectivity

### Email Issues

- **Gmail users**: Use an "App Password" instead of your regular password
- **SMTP errors**: Verify SMTP host, port, and credentials
- **Port 587**: Most SMTP servers use port 587 with STARTTLS

### Job Not Found

- Ensure the job name in config.toml exactly matches the Jenkins job name (case-sensitive)
- For jobs in folders, use the full path: `folder/job-name`

## MVP Limitations

This MVP version has the following limitations:
- **In-memory state only**: State is lost on restart
- **Email alerts only**: No Slack, PagerDuty, or other integrations yet
- **Basic scheduling logic**: Uses cron expressions for expected schedules
- **Single Jenkins instance**: Can only monitor one Jenkins server
- **No web UI**: Configuration is file-based only

These limitations will be addressed in future releases as outlined in [ROADMAP.md](ROADMAP.md).

## Example Output

```
[2025-12-06T03:00:00Z INFO  jenkins_monitor] Starting Jenkins Monitor
[2025-12-06T03:00:00Z INFO  jenkins_monitor] Loading configuration from: "config.toml"
[2025-12-06T03:00:00Z INFO  jenkins_monitor] Configuration loaded successfully
[2025-12-06T03:00:00Z INFO  jenkins_monitor] Jenkins client initialized for: https://jenkins.example.com
[2025-12-06T03:00:00Z INFO  jenkins_monitor] Testing Jenkins connection...
[2025-12-06T03:00:00Z INFO  jenkins_monitor] Jenkins connection successful
[2025-12-06T03:00:00Z INFO  jenkins_monitor] Starting monitoring loop...
[2025-12-06T03:00:00Z INFO  jenkins_monitor::monitor] Running monitoring check...
[2025-12-06T03:00:00Z INFO  jenkins_monitor::monitor] Checking job: nightly-build
```

## Next Steps

- Review [ARCHITECTURE.md](ARCHITECTURE.md) to understand the design
- Check [ROADMAP.md](ROADMAP.md) for planned features
- See [CONTRIBUTING.md](CONTRIBUTING.md) to contribute

## Support

For issues and questions:
- Open an issue on [GitHub Issues](https://github.com/jstarcher/jenkins-monitor/issues)
- Check existing issues for similar problems
