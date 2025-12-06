# Contributing to Jenkins Monitor

Thank you for your interest in contributing to Jenkins Monitor! This document provides guidelines and instructions for contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
- [Development Setup](#development-setup)
- [How to Contribute](#how-to-contribute)
- [Coding Standards](#coding-standards)
- [Testing Guidelines](#testing-guidelines)
- [Pull Request Process](#pull-request-process)
- [Release Process](#release-process)

## Code of Conduct

This project adheres to a Code of Conduct that all contributors are expected to follow. Please read [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) before contributing.

## Getting Started

### Prerequisites

- **Rust**: Install Rust using [rustup](https://rustup.rs/)
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Git**: Version control system
- **Jenkins Instance**: For testing (can use Docker)

### Finding Issues to Work On

- Look for issues labeled `good first issue` for beginner-friendly tasks
- Check `help wanted` label for issues where contributions are especially welcome
- Review the [ROADMAP.md](ROADMAP.md) for upcoming features

## Development Setup

1. **Fork and Clone**
   ```bash
   git clone https://github.com/YOUR_USERNAME/jenkins-monitor.git
   cd jenkins-monitor
   ```

2. **Build the Project**
   ```bash
   cargo build
   ```

3. **Run Tests**
   ```bash
   cargo test
   ```

4. **Run Locally**
   ```bash
   cargo run -- --help
   ```

### Setting Up a Test Jenkins Instance

Using Docker:
```bash
docker run -p 8080:8080 -p 50000:50000 \
  -v jenkins_home:/var/jenkins_home \
  jenkins/jenkins:lts
```

Access Jenkins at http://localhost:8080

## How to Contribute

### Reporting Bugs

When reporting bugs, please include:
- Rust version (`rustc --version`)
- Operating system and version
- Jenkins version
- Steps to reproduce
- Expected vs actual behavior
- Relevant logs or error messages

Use the bug report template when creating an issue.

### Suggesting Enhancements

Enhancement suggestions are welcome! Please:
- Check if the feature is already in the [ROADMAP.md](ROADMAP.md)
- Provide clear use cases
- Explain why this feature would be useful
- Consider implementation complexity

### Submitting Changes

1. Create a new branch from `main`
   ```bash
   git checkout -b feature/your-feature-name
   ```

2. Make your changes following our [coding standards](#coding-standards)

3. Add tests for new functionality

4. Ensure all tests pass
   ```bash
   cargo test
   ```

5. Run the linter and formatter
   ```bash
   cargo clippy -- -D warnings
   cargo fmt --check
   ```

6. Commit your changes with clear commit messages
   ```bash
   git commit -m "Add feature: brief description"
   ```

7. Push to your fork
   ```bash
   git push origin feature/your-feature-name
   ```

8. Open a Pull Request

## Coding Standards

### Rust Style Guide

- Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- Use `cargo fmt` to format code (enforced in CI)
- Use `cargo clippy` to catch common mistakes
- Maximum line length: 100 characters
- Use meaningful variable and function names

### Code Organization

```
jenkins-monitor/
├── src/
│   ├── main.rs           # Application entry point
│   ├── lib.rs            # Library root
│   ├── api/              # Jenkins API client
│   ├── monitor/          # Monitoring logic
│   ├── alert/            # Alert management
│   ├── config/           # Configuration handling
│   └── utils/            # Utility functions
├── tests/                # Integration tests
├── examples/             # Example usage
└── benches/              # Performance benchmarks
```

### Documentation

- Document all public APIs with doc comments (`///`)
- Include examples in doc comments where appropriate
- Use `cargo doc --open` to preview documentation
- Update README.md for significant changes

Example:
```rust
/// Fetches the status of a Jenkins job.
///
/// # Arguments
///
/// * `job_name` - The name of the Jenkins job
///
/// # Returns
///
/// Returns a `Result` containing the `JobStatus` or an error.
///
/// # Examples
///
/// ```
/// let status = client.get_job_status("my-job").await?;
/// println!("Job status: {:?}", status);
/// ```
pub async fn get_job_status(&self, job_name: &str) -> Result<JobStatus> {
    // Implementation
}
```

### Error Handling

- Use `Result<T, E>` for operations that can fail
- Create custom error types with `thiserror`
- Provide helpful error messages
- Use `?` operator for error propagation

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MonitorError {
    #[error("Failed to connect to Jenkins: {0}")]
    ConnectionError(String),
    
    #[error("Job '{0}' not found")]
    JobNotFound(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(#[from] ConfigError),
}
```

## Testing Guidelines

### Unit Tests

- Write unit tests for individual functions
- Place tests in the same file as the code (in a `tests` module)
- Use descriptive test names

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cron_expression() {
        let expr = "0 9 * * 1-5";
        let result = parse_cron(expr);
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_cron_expression() {
        let expr = "invalid";
        let result = parse_cron(expr);
        assert!(result.is_err());
    }
}
```

### Integration Tests

- Place integration tests in the `tests/` directory
- Use realistic test scenarios
- Mock external Jenkins API calls when appropriate

### Test Coverage

- Aim for >80% code coverage
- All new features must include tests
- Bug fixes should include regression tests

```bash
# Install tarpaulin for coverage reports
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html
```

## Pull Request Process

### Before Submitting

- [ ] Code follows style guidelines
- [ ] Self-review completed
- [ ] Comments added for complex logic
- [ ] Documentation updated
- [ ] Tests added/updated
- [ ] All tests pass locally
- [ ] No clippy warnings
- [ ] Code formatted with `cargo fmt`

### PR Description Template

```markdown
## Description
Brief description of changes

## Related Issue
Fixes #123

## Type of Change
- [ ] Bug fix
- [ ] New feature
- [ ] Breaking change
- [ ] Documentation update

## Testing
How has this been tested?

## Checklist
- [ ] Tests added
- [ ] Documentation updated
- [ ] No breaking changes (or documented)
```

### Review Process

1. At least one maintainer review required
2. All CI checks must pass
3. Address review feedback
4. Maintainer merges once approved

## Release Process

(For Maintainers)

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md`
3. Create release commit
4. Tag the release: `git tag -a v1.0.0 -m "Release v1.0.0"`
5. Push tag: `git push origin v1.0.0`
6. GitHub Actions will build and publish release artifacts

## Questions or Need Help?

- Open a [GitHub Discussion](https://github.com/jstarcher/jenkins-monitor/discussions)
- Ask in issue comments
- Contact maintainers

## Recognition

Contributors will be recognized in:
- README.md contributors section
- Release notes
- Project documentation

Thank you for contributing to Jenkins Monitor! 🎉
