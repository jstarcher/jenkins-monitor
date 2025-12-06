# Quick Start Guide

## 🚀 Get Started in 5 Minutes

### 1. Build
```bash
cargo build --release
```

### 2. Configure
```bash
cp config.example.toml config.toml
# Edit config.toml with your Jenkins and email settings
```

### 3. Run
```bash
./target/release/jenkins-monitor
```

## Minimum Required Configuration

```toml
[general]
log_level = "info"
check_interval_seconds = 60

[jenkins]
url = "https://your-jenkins.com"
username = "your-username"
password = "your-api-token"

[[job]]
name = "your-job-name"
schedule = "0 0 2 * * *"  # When job should run
alert_threshold_minutes = 90  # How long to wait before alerting

# Optional: Email alerts
[email]
smtp_host = "smtp.gmail.com"
smtp_port = 587
from = "monitor@example.com"
to = ["team@example.com"]
username = "your-email@gmail.com"
password = "your-app-password"
```

## What It Does

1. Checks Jenkins every 60 seconds
2. Verifies each job ran according to its schedule
3. Sends email alert if a job misses its expected run time

That's it! Simple monitoring, simple alerts. Ship it! 🚢

See [USAGE.md](USAGE.md) for detailed documentation.
