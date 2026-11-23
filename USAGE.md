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
password = "your-api-token"
# Time zone your Jenkins controller runs cron triggers in (default UTC)
timezone = "America/New_York"
```

**Important:** Use a Jenkins API token instead of your password. You can generate one in Jenkins at:
`Your Name → Configure → API Token`. The Jenkins user only needs read access to the jobs.

To keep credentials out of the file, leave `username`/`password` out and set
`JENKINS_MONITOR_JENKINS_USERNAME` and `JENKINS_MONITOR_JENKINS_PASSWORD` in the environment.

### Step 3: Define Jobs to Monitor

Add a `[[job]]` section for each job you want to monitor:

```toml
[[job]]
name = "my-job-name"            # folders: "folder/subfolder/my-job-name"
schedule = "H 2 * * *"          # optional, see below
alert_threshold_minutes = 60    # grace period after each scheduled time
# alert_on_failure = true       # alert when the last completed build didn't succeed
# alert_on_error = true         # override general.alert_on_check_error
```

`schedule` is optional. Leave it out and the monitor reads the job's
"Build periodically" timer trigger from Jenkins on every check, so the monitor
follows changes made in Jenkins. An SCM polling spec is not treated as a
schedule, because polling only builds when something changed.

`alert_threshold_minutes` is how long after a scheduled time a build may start
before that run counts as missed. A daily 02:00 job with a threshold of 60 is
reported missed at 03:00 if no build started after 02:00.

#### Schedule Format

Schedules use Jenkins syntax, the same text you'd put in "Build periodically":

```
┌───────── minute (0 - 59)
│ ┌───────── hour (0 - 23)
│ │ ┌───────── day of month (1 - 31)
│ │ │ ┌───────── month (1 - 12)
│ │ │ │ ┌───────── day of week (0 - 7, 0 and 7 are Sunday, or MON-SUN)
│ │ │ │ │
* * * * *
```

Supported: `H`, `H/15`, `H(0-29)`, `@hourly`, `@daily`, `@midnight`, `@weekly`,
`@monthly`, `@yearly`, several lines (the job fires on any of them), `#` comments
and a `TZ=Area/City` line.

`H` is a per-job value Jenkins derives from a hash of the job name. The monitor
can't compute Jenkins' hash, so it allows for the whole range `H` could pick:
`H 2 * * *` is treated as "some time between 02:00 and 02:59", and the threshold
counts from 02:59. Alerts for `H` jobs can come a little later, never falsely.

Older 6-field specs with seconds first (`0 0 2 * * *`) still work.

**Examples:**
- `H 2 * * *` - once a day between 2:00 and 2:59
- `0 */2 * * *` - every 2 hours
- `30 9 * * 1-5` - 9:30 AM Monday through Friday
- `0 0 1 * *` - first day of every month at midnight
- `@daily` - once a day at a hashed time

### Step 4: Configure Email Alerts

To enable email alerts, add the `[email]` section:

```toml
[email]
smtp_host = "smtp.gmail.com"
smtp_port = 587
smtp_security = "starttls"      # starttls (587), tls (465) or none (local relay only)
from = "jenkins-monitor@example.com"
to = ["team@example.com", "admin@example.com"]
username = "your-email@gmail.com"
password = "your-app-password"
```

`username`/`password` can also come from `JENKINS_MONITOR_SMTP_USERNAME` and
`JENKINS_MONITOR_SMTP_PASSWORD`. Without an `[email]` section, alerts are only logged.

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
log_level = "info"             # error, warn, info, debug, trace
check_interval_seconds = 60    # how often to check jobs
alert_on_check_error = true    # alert when the monitor can't check a job...
error_alert_after = 3          # ...after this many failed checks in a row
realert_minutes = 0            # repeat unresolved alerts every N minutes (0 = once)
send_recovery = true           # email when a job is healthy again
```

Unknown keys are rejected, so a typo fails at startup instead of being ignored.

## Running the Monitor

### Basic Usage

```bash
# Check the config and see each job's schedule (reads schedules from Jenkins)
./target/release/jenkins-monitor --config /path/to/config.toml --check

# Run continuously
./target/release/jenkins-monitor --config /path/to/config.toml

# Run one check and exit, for cron or a Kubernetes CronJob
./target/release/jenkins-monitor --once
```

The config path defaults to `$JENKINS_MONITOR_CONFIG`, then `./config.toml`.
With `--once` nothing is remembered between runs, so an open problem is
alerted on every run and `error_alert_after` has no effect; prefer the
long-running mode when you can.

### Run in Background

```bash
# Linux/macOS
nohup ./target/release/jenkins-monitor > jenkins-monitor.log 2>&1 &
```

### Using with systemd (Linux)

`make install-service` installs the binary and `packaging/jenkins-monitor.service`,
which runs `jenkins-monitor --config /etc/jenkins-monitor/config.toml` as the
`jenkins-monitor` user. Put secrets in `/etc/jenkins-monitor/env` (chmod 600):

```bash
JENKINS_MONITOR_JENKINS_PASSWORD=your-api-token
JENKINS_MONITOR_SMTP_PASSWORD=your-app-password
```

Then:

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now jenkins-monitor
sudo systemctl status jenkins-monitor
```

## Understanding Alerts

A job needs attention when:

- **Missed run:** a scheduled time plus `alert_threshold_minutes` has passed and no build started after that scheduled time.
- **Never built:** a run was due and the job has no builds at all.
- **Failed:** the last *completed* build finished with anything other than SUCCESS (FAILURE, UNSTABLE, ABORTED...). A build still running doesn't hide this.
- **Check error:** the monitor couldn't check the job (Jenkins unreachable, 404, bad credentials, unreadable schedule) for `error_alert_after` checks in a row.

Each problem is emailed once, and again every `realert_minutes` if you set it.
When the job is healthy again you get a "recovered" email. A new failed build
is a new problem; a job stuck without running is one problem however many
runs it misses.

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


### Alert Email Format

```
Subject: Jenkins Monitor: 'nightly-build' needs attention

Jenkins Monitor alert for job 'nightly-build'

- Missed scheduled run at 2025-12-06 02:00 UTC. Last build was #41 at 2025-12-05 02:13 UTC.

Schedule: H 2 * * *
Time zone: UTC
Threshold: 60 minutes
Job: https://jenkins.example.com/job/nightly-build
Checked: 2025-12-06 04:00:00 UTC
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

### "cannot read config file"

Pass the path with `--config`, set `JENKINS_MONITOR_CONFIG`, or run from the
directory that holds `config.toml`.

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

- Run with `--check` to see the schedule the monitor is using
- Make sure `jenkins.timezone` matches the Jenkins controller's time zone
- Check the `alert_threshold_minutes` is long enough for queue time
- Use `log_level = "debug"` to see each request

### "the job has no timer trigger in Jenkins"

The job has no "Build periodically" trigger (perhaps it's triggered by SCM
polling or another job). Set `schedule` for it in `config.toml`.

## Best Practices

1. **Start Simple**: Monitor 1-2 jobs first and run `--check`
2. **Set Appropriate Thresholds**: The threshold is the delay you'll tolerate after each scheduled time, not the interval between runs
3. **Use API Tokens**: Never use your actual password in the config
4. **Regular Testing**: Manually test that alerts work
5. **Monitor the Monitor**: Ensure the monitor itself is running reliably

## Security Considerations

- Store `config.toml` with restricted permissions: `chmod 600 config.toml` (the monitor warns if it's world-readable)
- Or keep secrets out of it with the `JENKINS_MONITOR_*` environment variables; the systemd unit reads them from `/etc/jenkins-monitor/env`
- Use Jenkins API tokens instead of passwords
- Keep the Jenkins user's permissions minimal (read-only is sufficient)

## Examples

### Example 1: Monitor Nightly Builds

```toml
[[job]]
name = "nightly-build"
schedule = "H 2 * * *"   # once between 2:00 and 2:59
alert_threshold_minutes = 60
```

### Example 2: Monitor Hourly Health Checks

```toml
[[job]]
name = "health-check"
schedule = "0 * * * *"   # on the hour
alert_threshold_minutes = 15
```

### Example 3: Monitor Weekly Reports

```toml
[[job]]
name = "weekly-report"
schedule = "0 8 * * 1"   # Monday 8 AM
alert_threshold_minutes = 120
```

## Getting Help

If you encounter issues:

1. Check the logs with `log_level = "debug"`
2. Review this documentation
3. Open an issue on GitHub: https://github.com/jstarcher/jenkins-monitor/issues
4. Include your configuration (with sensitive data removed) and log output

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for details.
