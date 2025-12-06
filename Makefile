# Makefile for common development tasks for jenkins-monitor
# Usage: make <target>
#
# Targets:
#   help       - show this help
#   build      - build release binary (cargo build --release)
#   run        - run the release binary (builds first)
#   test       - run all tests (cargo test)
#   check      - run cargo check
#   fmt        - format the code (cargo fmt)
#   clippy     - run cargo clippy for linting
#   install    - install the binary into cargo bin directory
#   clean      - cargo clean
#   dist       - build release binary and create a tarball in target/

.PHONY: help build run test check fmt clippy install clean dist

help:
	@echo "Available make targets:"
	@echo "  help      - show this help"
	@echo "  build     - build release binary (cargo build --release)"
	@echo "  run       - run the release binary (builds first)"
	@echo "  test      - run unit/integration tests (cargo test)"
	@echo "  check     - run cargo check"
	@echo "  fmt       - format sources (cargo fmt)"
	@echo "  clippy    - run clippy (cargo clippy -- -D warnings)"
	@echo "  install   - install binary to cargo bin (cargo install --path .)"
	@echo "  clean     - clean build artifacts (cargo clean)"
	@echo "  dist      - produce a release tarball under target/"

build:
	cargo build --release

run: build
	./target/release/jenkins-monitor

test:
	cargo test

check:
	cargo check

fmt:
	cargo fmt --all

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

install:
	cargo install --path .

clean:
	cargo clean

dist: build
	@mkdir -p target/dist
	@tar -C target/release -czf target/dist/jenkins-monitor-$(shell git describe --tags --always)-$(shell date +%Y%m%d%H%M%S).tar.gz jenkins-monitor
	@echo "Created archive: target/dist/"
