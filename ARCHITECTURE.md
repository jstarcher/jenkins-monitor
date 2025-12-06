# Architecture Overview

## System Design

Jenkins Monitor is designed as a lightweight, standalone monitoring application that periodically checks Jenkins job execution status and sends alerts when jobs don't run as expected.

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                     Jenkins Monitor                          │
│                                                              │
│  ┌──────────────┐      ┌──────────────┐    ┌─────────────┐ │
│  │              │      │              │    │             │ │
│  │ Configuration│─────▶│   Scheduler  │───▶│  Monitor    │ │
│  │   Loader     │      │              │    │   Engine    │ │
│  │              │      │              │    │             │ │
│  └──────────────┘      └──────────────┘    └─────┬───────┘ │
│                                                   │         │
│                                                   ▼         │
│                                          ┌─────────────┐   │
│                                          │   Jenkins   │   │
│                                          │ API Client  │   │
│                                          └──────┬──────┘   │
│                                                 │          │
│  ┌──────────────┐      ┌──────────────┐        │          │
│  │              │      │              │        │          │
│  │   Alerting   │◀─────│    State     │◀───────┘          │
│  │   Manager    │      │   Tracker    │                   │
│  │              │      │              │                   │
│  └──────┬───────┘      └──────────────┘                   │
│         │                                                  │
└─────────┼──────────────────────────────────────────────────┘
          │
          ▼
┌─────────────────────┐
│ Alert Destinations  │
│                     │
│ • Email (SMTP)      │
│ • Slack             │
│ • PagerDuty         │
│ • Webhooks          │
└─────────────────────┘
```

## Core Components

### 1. Configuration Loader

**Purpose**: Loads and validates configuration from files (TOML/YAML/JSON)

**Responsibilities**:
- Parse configuration files
- Validate configuration schema
- Provide configuration to other components
- Support hot-reload of configuration (future)

**Key Data Structures**:
```rust
struct Config {
    jenkins: JenkinsConfig,
    jobs: Vec<JobMonitorConfig>,
    alerts: AlertConfig,
}

struct JenkinsConfig {
    url: String,
    username: Option<String>,
    api_token: Option<String>,
    timeout: Duration,
}

struct JobMonitorConfig {
    name: String,
    expected_schedule: CronExpression,
    alert_threshold: Duration,
    enabled: bool,
}
```

### 2. Scheduler

**Purpose**: Orchestrates periodic monitoring checks

**Responsibilities**:
- Schedule periodic job checks
- Trigger monitoring engine at configured intervals
- Handle graceful shutdown
- Manage concurrent checks

**Implementation**:
- Use tokio for async scheduling
- Default check interval: 60 seconds (configurable)
- Exponential backoff on failures

### 3. Monitor Engine

**Purpose**: Core monitoring logic

**Responsibilities**:
- Determine which jobs should have run
- Compare expected vs actual job executions
- Detect anomalies and missing runs
- Calculate job health metrics

**Algorithm**:
```
For each monitored job:
  1. Get last build timestamp from Jenkins
  2. Calculate expected execution times based on cron schedule
  3. Compare actual vs expected
  4. If deviation > threshold:
     - Record issue
     - Trigger alert via Alert Manager
  5. Update job state in State Tracker
```

### 4. Jenkins API Client

**Purpose**: Interface with Jenkins REST API

**Responsibilities**:
- HTTP communication with Jenkins
- Authentication handling
- API response parsing
- Rate limiting and retry logic
- Connection pooling

**API Endpoints Used**:
- `/api/json` - General info
- `/job/{name}/api/json` - Job details
- `/job/{name}/lastBuild/api/json` - Last build info
- `/job/{name}/builds/api/json` - Build history

**Error Handling**:
- Network timeouts
- Authentication failures
- API rate limits
- Jenkins unavailability

### 5. State Tracker

**Purpose**: Maintain monitoring state and history

**Responsibilities**:
- Track last known job states
- Store alert history
- Persist state across restarts
- Provide query interface for reports

**Storage Options**:
- In-memory (default for simple deployments)
- SQLite (for persistence)
- PostgreSQL (for enterprise deployments)

**Data Model**:
```rust
struct JobState {
    job_name: String,
    last_check: DateTime<Utc>,
    last_build_timestamp: Option<DateTime<Utc>>,
    last_build_number: Option<u64>,
    last_build_status: Option<BuildStatus>,
    consecutive_misses: u32,
}

struct AlertRecord {
    id: Uuid,
    job_name: String,
    timestamp: DateTime<Utc>,
    alert_type: AlertType,
    message: String,
    resolved: bool,
}
```

### 6. Alerting Manager

**Purpose**: Handle alert generation and distribution

**Responsibilities**:
- Format alert messages
- Route alerts to appropriate channels
- Implement retry logic for failed alerts
- De-duplicate alerts
- Track alert state (sent, acknowledged, resolved)

**Alert Types**:
- `JobMissed`: Job didn't run when expected
- `JobFailed`: Job ran but failed
- `JobDelayed`: Job ran but later than expected
- `JenkinsUnreachable`: Can't connect to Jenkins

**Alert Channels**:
Each channel implements the `AlertChannel` trait:
```rust
#[async_trait]
trait AlertChannel {
    async fn send_alert(&self, alert: &Alert) -> Result<()>;
    fn channel_name(&self) -> &str;
}
```

## Data Flow

### Monitoring Cycle

```
1. Scheduler triggers check
2. Monitor Engine:
   a. Fetch job list from config
   b. For each job, query Jenkins API Client
   c. Compare with expected schedule
   d. Identify anomalies
3. State Tracker:
   a. Update job states
   b. Record any anomalies
4. Alert Manager:
   a. Generate alerts for anomalies
   b. Send to configured channels
   c. Record alert in State Tracker
```

### Alert Flow

```
Anomaly Detected
    │
    ▼
Create Alert Object
    │
    ├─▶ Check De-duplication
    │        │
    │        └─▶ [If duplicate, skip]
    │
    ├─▶ Format Message
    │
    └─▶ Send to Channels
            │
            ├─▶ Email
            ├─▶ Slack
            ├─▶ PagerDuty
            └─▶ Webhook
```

## Technology Stack

### Core Libraries

| Purpose | Library | Rationale |
|---------|---------|-----------|
| Async Runtime | tokio | Industry standard, excellent performance |
| HTTP Client | reqwest | Easy to use, async support, feature-rich |
| Serialization | serde | De-facto standard for Rust serialization |
| CLI Parsing | clap | Powerful, derive macros for ergonomics |
| Logging | tracing | Structured logging, async-aware |
| Error Handling | thiserror | Simple, idiomatic error definitions |
| Cron Parsing | cron | Robust cron expression parser |
| Config Format | toml, serde_yaml, serde_json | Support multiple formats |

### Optional Libraries

| Purpose | Library | When Used |
|---------|---------|-----------|
| Database | sqlx | When persistence is needed |
| Web Server | axum | For optional REST API/UI |
| Metrics | prometheus | For metrics export |
| Email | lettre | For email alerts |

## Configuration Example

```toml
[jenkins]
url = "https://jenkins.example.com"
username = "monitor"
api_token = "${JENKINS_API_TOKEN}"  # Environment variable
timeout = "30s"

[[jobs]]
name = "nightly-build"
expected_schedule = "0 2 * * *"  # Daily at 2 AM
alert_threshold = "1h"
enabled = true

[[jobs]]
name = "integration-tests"
expected_schedule = "0 */4 * * *"  # Every 4 hours
alert_threshold = "30m"
enabled = true

[alerts]
default_channels = ["email", "slack"]

[alerts.email]
smtp_host = "smtp.example.com"
smtp_port = 587
from = "jenkins-monitor@example.com"
to = ["ops-team@example.com"]

[alerts.slack]
webhook_url = "${SLACK_WEBHOOK_URL}"
channel = "#jenkins-alerts"
```

## Deployment Models

### Standalone Binary

```bash
# Run as a service
jenkins-monitor --config /etc/jenkins-monitor/config.toml
```

### Docker Container

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/jenkins-monitor /usr/local/bin/
CMD ["jenkins-monitor"]
```

### Kubernetes Deployment

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: jenkins-monitor
spec:
  replicas: 1
  template:
    spec:
      containers:
      - name: jenkins-monitor
        image: jenkins-monitor:latest
        volumeMounts:
        - name: config
          mountPath: /etc/jenkins-monitor
```

## Performance Considerations

### Scalability Targets

- Monitor up to 1000 jobs per instance
- Check interval: 60 seconds default
- Memory footprint: < 100MB
- CPU usage: < 5% during steady state

### Optimization Strategies

1. **Batching**: Group Jenkins API calls where possible
2. **Caching**: Cache job configurations and metadata
3. **Concurrency**: Parallel job checks using tokio
4. **Connection Pooling**: Reuse HTTP connections
5. **Incremental Checks**: Only check jobs due for execution

## Security Considerations

### Credential Management

- Support environment variables for secrets
- Integration with HashiCorp Vault (future)
- Never log sensitive credentials
- Encrypted storage for API tokens

### Network Security

- HTTPS for Jenkins communication
- TLS for SMTP connections
- Webhook signature verification
- Configurable certificate validation

### Access Control

- Read-only Jenkins API access required
- Principle of least privilege
- Audit logging for configuration changes

## Extensibility

### Plugin System (Future)

Allow custom alert channels and monitoring logic:

```rust
trait MonitorPlugin {
    fn name(&self) -> &str;
    fn on_job_check(&self, job: &JobState) -> Result<()>;
    fn on_alert(&self, alert: &Alert) -> Result<()>;
}
```

### API for Integration

REST API for querying state and configuration:
- `GET /api/v1/status` - Overall status
- `GET /api/v1/jobs` - List monitored jobs
- `GET /api/v1/jobs/{name}` - Job details
- `GET /api/v1/alerts` - Recent alerts

## Testing Strategy

### Unit Tests
- Test individual components in isolation
- Mock Jenkins API responses
- Test configuration parsing
- Test alert formatting

### Integration Tests
- Test full monitoring cycle with test Jenkins
- Test various failure scenarios
- Test alert delivery

### Performance Tests
- Benchmark API call overhead
- Test with 1000+ jobs
- Memory leak detection
- Load testing alert channels

## Monitoring the Monitor

### Health Checks
- HTTP health endpoint: `/health`
- Check Jenkins connectivity
- Verify alert channel availability
- Report internal errors

### Metrics
- Jobs checked per minute
- Alerts sent per minute
- Jenkins API response times
- Alert delivery success rate
- Memory and CPU usage

---

**Last Updated**: 2025-12-06
