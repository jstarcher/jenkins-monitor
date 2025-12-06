#!/usr/bin/env bash
set -euo pipefail

# Build a list of Jenkins jobs via the Jenkins HTTP API.
#
# Usage:
#   JENKINS_URL=https://jenkins.example.com \
#   JENKINS_USER=me \
#   JENKINS_TOKEN=token \
#   ./scripts/build_jenkins_jobs.sh [job1 job2 ...]
#
# If no job names are passed on the command line the script builds the
# three default jobs used by this repository's example configuration:
#   nightly-build, hourly-tests, integration-tests
#
# The script supports jobs that live in folders (slash-separated paths). It
# automatically requests a Jenkins crumb (CSRF token) if Jenkins requires one.

DEFAULT_JOBS=( "nightly-build" "hourly-tests" "integration-tests" )

if [ "$#" -gt 0 ]; then
  JOBS=("$@")
else
  JOBS=("${DEFAULT_JOBS[@]}")
fi

JENKINS_URL=${JENKINS_URL:-}
JENKINS_USER=${JENKINS_USER:-}
JENKINS_TOKEN=${JENKINS_TOKEN:-}

if [ -z "$JENKINS_URL" ] || [ -z "$JENKINS_USER" ] || [ -z "$JENKINS_TOKEN" ]; then
  cat <<EOF
Missing required environment variables.

Set these environment variables and re-run the script:
  JENKINS_URL   e.g. https://jenkins.example.com
  JENKINS_USER  Jenkins username
  JENKINS_TOKEN Jenkins API token (or password if configured)

Example:
  JENKINS_URL=https://jenkins.example.com \
  JENKINS_USER=me \
  JENKINS_TOKEN=token \
  ./scripts/build_jenkins_jobs.sh

EOF
  exit 1
fi

# If no jobs supplied on the command line attempt to read from config.toml in the repo root.
# This ensures the script uses the exact job paths (including foldered paths) configured for the
# monitor rather than falling back to potentially incorrect hard-coded defaults.
if [ "$#" -eq 0 ]; then
  if [ -f "../config.toml" ]; then
    echo "No jobs given on command-line — reading job names from ../config.toml"
    mapfile -t cfg_jobs < <(awk '/^\[\[job\]\]/{capture=1;next} capture && /name\s*=/{gsub(/^[ \t]+|[ \t]+$/, "", $0); split($0,a,"=",2); gsub(/^[ \t\"]+|[ \t\"]+$/,"", a[2]); print a[2]} /^\[\[/{if($0!~/^\[\[job\]\]/){capture=0}}' ../config.toml)
    if [ ${#cfg_jobs[@]} -gt 0 ]; then
      JOBS=("${cfg_jobs[@]}")
    else
      echo "Warning: no jobs found in ../config.toml; using defaults"
    fi
  fi
fi

# Normalize base (strip trailing slash)
BASE="${JENKINS_URL%/}"

# Support DRY_RUN mode: if enabled we skip network operations like crumb fetch
DRY=${DRY_RUN:-0}

# Try to fetch crumb (if Jenkins has CSRF protection enabled) — skip when DRY_RUN
CRUMB_HEADER=""
crumb=""
crumb_field=""
if [ "${DRY}" = "0" ]; then
  CRUMB_ENDPOINT="${BASE}/crumbIssuer/api/json"
  # Silence curl errors when retrieving crumb (e.g. in CI or example host)
  if crumb_json=$(curl -s --user "${JENKINS_USER}:${JENKINS_TOKEN}" "${CRUMB_ENDPOINT}" 2>/dev/null || true); then
    if [ -n "${crumb_json}" ]; then
      crumb=$(echo "$crumb_json" | awk -F '"' '/crumb"/ {print $4; exit}')
      crumb_field=$(echo "$crumb_json" | awk -F '"' '/crumbRequestField"/ {print $4; exit}')
      if [ -n "$crumb" ] && [ -n "$crumb_field" ]; then
        CRUMB_HEADER="${crumb_field}: ${crumb}"
        echo "Found crumb: ${crumb_field}=<redacted>"
      fi
    fi
  fi
else
  echo "DRY_RUN enabled; skipping crumb retrieval"
fi

echo "Triggering builds for: ${JOBS[*]}"

for job in "${JOBS[@]}"; do
  # Build the API URL for this job. For nested jobs Jenkins requires /job/seg1/job/seg2/.../build
  job_url="$BASE"
  IFS='/' read -ra parts <<<"$job"
  for p in "${parts[@]}"; do
    enc=$(python3 -c "import urllib.parse, sys; print(urllib.parse.quote(sys.argv[1], safe=''))" "$p")
    job_url+="/job/${enc}"
  done
  build_url="${job_url}/build"

  echo "-> Triggering: ${build_url}"

  # Allow a dry-run via DRY_RUN=1 environment var
  if [ "${DRY_RUN:-0}" != "0" ]; then
    echo "DRY_RUN enabled; would POST to: ${build_url}"
    echo "  (DRY_RUN) would check existence: ${job_url}/api/json"
    echo "  (DRY_RUN) job not found -> would create folders and job if needed"
    continue
  fi

  # Check job existence; if 404 create any missing parent folders and the job itself
  check_url="${job_url}/api/json"
  exist_code=$(curl -s -o /dev/null -w "%{http_code}" --user "${JENKINS_USER}:${JENKINS_TOKEN}" "${check_url}" || true)
  if [ "${exist_code}" = "404" ]; then
    echo "  Job '${job}' not found (HTTP 404). Creating job and any missing folders."
    parent_url="$BASE"
    IFS='/' read -ra parts2 <<<"$job"
    last_index=$((${#parts2[@]} - 1))
    for idx in "${!parts2[@]}"; do
      name=${parts2[$idx]}
      enc=$(python3 -c "import urllib.parse, sys; print(urllib.parse.quote(sys.argv[1], safe=''))" "$name")
      if [ $idx -lt $last_index ]; then
        # folder check: parent_url/job/enc/api/json
        folder_check_url="${parent_url}/job/${enc}/api/json"
        folder_exists=$(curl -s -o /dev/null -w "%{http_code}" --user "${JENKINS_USER}:${JENKINS_TOKEN}" "${folder_check_url}" || true)
        if [ "${folder_exists}" = "404" ]; then
          echo "  Folder '${name}' not found - creating it under ${parent_url}"
          create_endpoint="${parent_url}/createItem?name=${enc}"
          folder_xml="<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<com.cloudbees.hudson.plugins.folder.Folder plugin=\"cloudbees-folder@6.15\">\n  <actions/>\n  <description>Created by jenkins-monitor</description>\n  <properties/>\n  <folderViews class=\"com.cloudbees.hudson.plugins.folder.views.FolderViewHolder\">\n    <views/>\n    <primaryView>All</primaryView>\n  </folderViews>\n  <healthMetrics/>\n  <icon class=\"com.cloudbees.hudson.plugins.folder.icons.StockFolderIcon\"/>\n</com.cloudbees.hudson.plugins.folder.Folder>"
          if [ -n "${CRUMB_HEADER}" ]; then
            http_code=$(curl -s -o /dev/null -w "%{http_code}" --user "${JENKINS_USER}:${JENKINS_TOKEN}" -H "Content-Type: application/xml" -H "${CRUMB_HEADER}" -X POST --data-binary "$folder_xml" "${create_endpoint}")
          else
            http_code=$(curl -s -o /dev/null -w "%{http_code}" --user "${JENKINS_USER}:${JENKINS_TOKEN}" -H "Content-Type: application/xml" -X POST --data-binary "$folder_xml" "${create_endpoint}")
          fi
          if [ "$http_code" -ge 200 ] && [ "$http_code" -lt 400 ]; then
            echo "    Created folder '${name}' (HTTP ${http_code})"
          else
            echo "    Failed to create folder '${name}' (HTTP ${http_code})"
          fi
        else
          echo "  Folder '${name}' already exists (HTTP ${folder_exists})"
        fi
        # Descend into this folder
        parent_url+="/job/${enc}"
      else
        # final part -- actual job name to create under current parent_url
        create_endpoint="${parent_url}/createItem?name=${enc}"
        echo "  Creating job '${name}' using endpoint: ${create_endpoint}"
        job_xml="<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<flow-definition plugin=\"workflow-job@2.40\">\n  <description>Test job created by jenkins-monitor</description>\n  <keepDependencies>false</keepDependencies>\n  <properties/>\n  <definition class=\"org.jenkinsci.plugins.workflow.cps.CpsFlowDefinition\" plugin=\"workflow-cps@2.90\">\n    <script>echo 'Hello from ${name}'</script>\n    <sandbox>true</sandbox>\n  </definition>\n  <triggers/>\n  <disabled>false</disabled>\n</flow-definition>"
        if [ -n "${CRUMB_HEADER}" ]; then
          http_code=$(curl -s -o /dev/null -w "%{http_code}" --user "${JENKINS_USER}:${JENKINS_TOKEN}" -H "Content-Type: application/xml" -H "${CRUMB_HEADER}" -X POST --data-binary "$job_xml" "${create_endpoint}")
        else
          http_code=$(curl -s -o /dev/null -w "%{http_code}" --user "${JENKINS_USER}:${JENKINS_TOKEN}" -H "Content-Type: application/xml" -X POST --data-binary "$job_xml" "${create_endpoint}")
        fi
        if [ "$http_code" -ge 200 ] && [ "$http_code" -lt 400 ]; then
          echo "    Created job '${name}' (HTTP ${http_code})"
        else
          echo "    Failed to create job '${name}' (HTTP ${http_code})"
        fi
      fi
    done
  fi

  # Use -X POST and credentials. Include crumbs if available. Capture HTTP status and curl exit code.
  set +e
  curl_args=( -sS -o /dev/null -w "%{http_code}" -X POST --user "${JENKINS_USER}:${JENKINS_TOKEN}" )
  if [ -n "${CRUMB_HEADER}" ]; then
    curl_args+=( -H "${CRUMB_HEADER}" )
  fi
  curl_args+=( "${build_url}" )

  http_code=$(curl "${curl_args[@]}")
  rc=$?
  set -e

  if [ $rc -ne 0 ]; then
    echo "  Request failed (curl exit ${rc}) for '${job}'"
  elif [ -n "${http_code}" ] && [ "${http_code}" -ge 200 ] && [ "${http_code}" -lt 400 ]; then
    echo "  Request sent for '${job}' (HTTP ${http_code})"
  else
    echo "  Server returned HTTP ${http_code} when attempting to start '${job}'"
  fi
done

echo "Done."
