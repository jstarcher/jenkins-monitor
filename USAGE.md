# Jenkins Monitor - Usage Guide

## Overview

Jenkins Monitor is a simple tool that monitors your Jenkins jobs to ensure they run according to their expected schedule. When a job doesn't run when expected, it sends you an alert via email.

## Prerequisites

- Rust toolchain (if building from source)
- A Jenkins instance with API access
- SMTP server access (for email alerts)

## Installation

### From Source

```bash
git clone https://github.com/jstarcher/jenkins-monitor.git
cd jenkins-monitor
cargo build --release
```

The compiled binary will be located at `target/release/jenkins-monitor`

## Configuration

### Step 1: Create a Configuration File

Copy the example configuration:

```bash
cp config.example.toml config.toml
```

### Step 2: Configure Jenkins Connection

Edit `config.toml` and update the `[jenkins]` section:

```toml
[jenkins]
url = "https://your-jenkins-server.com"
username = "your-username"
password = "your-api-token-or-password"
```

**Important:** For security, use a Jenkins API token instead of your password. You can generate one in Jenkins at:
`Your Name → Configure → API Token`

### Step 3: Define Jobs to Monitor

Add a `[[job]]` section for each job you want to monitor:

```toml
[[job]]
name = "my-job-name"
schedule = "0 0 2 * * *"  # Daily at 2 AM
alert_threshold_minutes = 90  # Alert if not run in 90 minutes

Note: If your Jenkins job is inside one or more folders, specify the path with forward slashes. For example:

```toml
[[job]]
name = "folder/subfolder/my-job-name"
```
The monitor will translate this into the Jenkins API path `/job/folder/job/subfolder/job/my-job-name/api/json`.
```

#### Understanding Cron Schedule Format

The schedule uses cron format with seconds:

```
┌─────── second (0 - 59)
│ ┌─────── minute (0 - 59)
│ │ ┌─────── hour (0 - 23)
│ │ │ ┌─────── day of month (1 - 31)
│ │ │ │ ┌─────── month (1 - 12)
│ │ │ │ │ ┌─────── day of week (0 - 6) (Sunday to Saturday)
│ │ │ │ │ │
* * * * * *
```

**Examples:**
- `0 0 2 * * *` - Daily at 2:00 AM
- `0 0 */2 * * *` - Every 2 hours
- `0 30 9 * * 1-5` - 9:30 AM Monday through Friday
- `0 0 0 1 * *` - First day of every month at midnight

### Step 4: Configure Email Alerts

To enable email alerts, add the `[email]` section:

```toml
[email]
smtp_host = "smtp.gmail.com"
smtp_port = 587
from = "jenkins-monitor@example.com"
to = ["team@example.com", "admin@example.com"]
username = "your-email@gmail.com"
password = "your-app-password"
```

#### Gmail Configuration

If using Gmail:
1. Enable 2-factor authentication on your Google account
2. Generate an App Password: https://myaccount.google.com/apppasswords
3. Use the App Password in the config (not your regular password)

#### Other SMTP Providers

Common SMTP settings:
- **Gmail**: smtp.gmail.com:587
- **Outlook/Office365**: smtp.office365.com:587
- **Yahoo**: smtp.mail.yahoo.com:587
- **SendGrid**: smtp.sendgrid.net:587

### Step 5: Adjust General Settings

```toml
[general]
log_level = "info"  # Options: error, warn, info, debug, trace
check_interval_seconds = 60  # How often to check jobs
```

## Running the Monitor

### Basic Usage

```bash
# Make sure config.toml is in the current directory
./target/release/jenkins-monitor
```

### Run in Background

```bash
# Linux/macOS
nohup ./target/release/jenkins-monitor > jenkins-monitor.log 2>&1 &
```

### Using with systemd (Linux)

Create `/etc/systemd/system/jenkins-monitor.service`:

```ini
[Unit]
Description=Jenkins Monitor
After=network.target

[Service]
Type=simple
User=jenkins-monitor
WorkingDirectory=/opt/jenkins-monitor
ExecStart=/opt/jenkins-monitor/jenkins-monitor
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

## Understanding Alerts

## Triggering builds from the command line

For convenience there's a small script included at `scripts/build_jenkins_jobs.sh` that triggers builds for the example jobs used in this repository.

Default jobs triggered:
- `nightly-build`
- `hourly-tests`
- `integration-tests`

Usage (recommended):

```bash
# Provide your Jenkins base URL, username and API token using environment variables
JENKINS_URL=https://jenkins.example.com \
JENKINS_USER=your-user \
JENKINS_TOKEN=your-token \
./scripts/build_jenkins_jobs.sh
```

You can also pass specific job names on the command-line instead of the defaults:

```bash
JENKINS_URL=... JENKINS_USER=... JENKINS_TOKEN=... \
./scripts/build_jenkins_jobs.sh "folder/subjob/one" "another-job"
```

Notes:
- The script supports foldered job paths using slash-separated names (e.g. `team/nightly-build`).
- By default the script performs real POSTs. Use `DRY_RUN=1` to only print what would be done.
- The script attempts to obtain a Jenkins crumb (CSRF token) automatically and will include it when required.

Creating missing jobs
----------------------

If a monitored job doesn't exist, `scripts/build_jenkins_jobs.sh` can create a minimal test job (and any missing folders) before triggering a build. This is useful when you're setting up a test Jenkins instance.

Important:
- The Jenkins user you supply must have permissions to create jobs and folders (usually admin or job-creation privileges).
- The script assumes the Jenkins instance has the CloudBees Folder plugin if you use foldered jobs (most modern Jenkins installations provide folder support).

If the script detects a missing job it will:
1. Create any missing folders in the path
2. Create a simple pipeline job that echoes the job name
3. Trigger a build of the newly-created job

Use `DRY_RUN=1` to preview the actions without making any network requests.


### When Alerts Are Sent

An alert is sent when:
1. A job hasn't run in longer than its `alert_threshold_minutes`
2. The job was expected to run based on its schedule
3. The monitoring check detected the missed execution

### Alert Email Format

Alerts include:
- Job name
- Description of the issue
- Timestamp of the alert
- Jenkins server URL

Example:
```
Subject: Jenkins Monitor Alert: nightly-build

Jenkins Monitor Alert

Job: nightly-build

Job hasn't run within expected schedule.
Expected schedule: 0 0 2 * * *
Alert threshold: 90 minutes

Time: 2025-12-06 03:45:00 UTC
Jenkins URL: https://jenkins.example.com
```

## Monitoring the Monitor

### Log Files

The monitor outputs logs based on the configured log level:

- **error**: Only errors
- **warn**: Warnings and errors
- **info**: General information (recommended)
- **debug**: Detailed debugging info
- **trace**: Very verbose output

### Checking if it's Running

```bash
# If running in background
ps aux | grep jenkins-monitor

# If using systemd
sudo systemctl status jenkins-monitor
```

### Viewing Logs

```bash
# If using systemd
sudo journalctl -u jenkins-monitor -f

# If running with nohup
tail -f jenkins-monitor.log
```

## Troubleshooting

### "Cannot find config file"

Make sure `config.toml` is in the same directory where you're running the command.

### "Failed to fetch job info from Jenkins"

- Check that the Jenkins URL is correct
- Verify your username and password/API token
- Ensure the job name matches exactly (case-sensitive)
- Check that your Jenkins user has permission to view the jobs
 - If your job lives in folders use a slash-separated path (e.g. `folder/subfolder/job`); the monitor converts these into the required `/job/.../api/json` path

### "Failed to send email"

- Verify SMTP host and port are correct
- Check username and password
- For Gmail, ensure you're using an App Password
- Check firewall rules allow outbound SMTP connections

### Job Always Shows as Missed

- Verify the cron schedule matches when the job actually runs in Jenkins
- Check the `alert_threshold_minutes` is appropriate for the schedule
- Use `log_level = "debug"` to see detailed timing information

## Best Practices

1. **Start Simple**: Monitor 1-2 jobs first to verify configuration
2. **Set Appropriate Thresholds**: Set `alert_threshold_minutes` to ~1.5x the schedule interval
3. **Use API Tokens**: Never use your actual password in the config
4. **Regular Testing**: Manually test that alerts work
5. **Monitor the Monitor**: Ensure the monitor itself is running reliably

## Security Considerations

- Store `config.toml` with restricted permissions: `chmod 600 config.toml`
- Use Jenkins API tokens instead of passwords
- Consider using environment variables for sensitive data (future feature)
- Keep the Jenkins user's permissions minimal (read-only is sufficient)

## Examples

### Example 1: Monitor Nightly Builds

```toml
[[job]]
name = "nightly-build"
schedule = "0 0 2 * * *"  # 2 AM daily
alert_threshold_minutes = 90
```

### Example 2: Monitor Hourly Health Checks

```toml
[[job]]
name = "health-check"
schedule = "0 0 * * * *"  # Every hour
alert_threshold_minutes = 75
```

### Example 3: Monitor Weekly Reports

```toml
[[job]]
name = "weekly-report"
schedule = "0 0 8 * * 1"  # Monday 8 AM
alert_threshold_minutes = 600  # 10 hours
```

## Getting Help

If you encounter issues:

1. Check the logs with `log_level = "debug"`
2. Review this documentation
3. Open an issue on GitHub: https://github.com/jstarcher/jenkins-monitor/issues
4. Include your configuration (with sensitive data removed) and log output

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for details.
