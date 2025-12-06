#!/usr/bin/env bash
set -euo pipefail

# Simple dry-run test for scripts/build_jenkins_jobs.sh

export JENKINS_URL=https://jenkins.example.com
export JENKINS_USER=testuser
export JENKINS_TOKEN=tokentest
export DRY_RUN=1

echo "Running dry-run test for build script..."
out=$(./scripts/build_jenkins_jobs.sh 2>&1)
echo "$out"

# Verify we skipped crumb fetch in DRY_RUN and would POST
if ! echo "$out" | grep -q "DRY_RUN enabled; skipping crumb retrieval"; then
	echo "Expected crumb skip message not found" >&2
	exit 2
fi

if ! echo "$out" | grep -q "DRY_RUN enabled; would POST to"; then
	echo "Expected DRY_RUN POST message not found" >&2
	exit 3
fi

if ! echo "$out" | grep -q "job not found -> would create folders and job if needed"; then
	echo "Expected job-creation dry-run message not found" >&2
	exit 4
fi

echo "Dry-run finished (expected)."
