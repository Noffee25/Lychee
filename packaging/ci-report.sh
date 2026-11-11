#!/usr/bin/env bash
# Write a GitHub Actions run's outcome to a branch of this repository:
#
#   packaging/ci-report.sh BRANCH TITLE LOGDIR
#
# RESULTS (env) holds "job=result" pairs; GH_TOKEN can push. The branch holds a single
# STATUS.md with the results and the end of every log under LOGDIR, so anyone (or any agent)
# with git access can check a run with `git fetch origin BRANCH && git show FETCH_HEAD:STATUS.md`.
# CI_REPORT_DRY=1 prints the report instead of pushing it.
set -euo pipefail

branch="${1:?branch}"
title="${2:?title}"
logs="${3:-logs}"
report="$(mktemp -d)"

{
    echo "# $title"
    echo
    echo "- commit: $GITHUB_SHA"
    echo "- run: $GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID"
    echo "- finished: $(date -u '+%Y-%m-%d %H:%M UTC')"
    for pair in $RESULTS; do echo "- ${pair%%=*}: **${pair#*=}**"; done
    if [ -d "$logs" ]; then
        find "$logs" -type f -name '*.log' | sort | while read -r log; do
            echo
            echo "## ${log#"$logs"/}"
            echo
            # Errors first, then the tail of the log.
            if grep -nE '^(error|warning: unused)|error\[|panicked|FAILED|Error:' "$log" | head -n 60 >"$report/hits"; [ -s "$report/hits" ]; then
                echo "Problems:"
                echo '```'
                cat "$report/hits"
                echo '```'
            fi
            echo "Last lines:"
            echo '```'
            tail -n 60 "$log"
            echo '```'
        done
    fi
} >"$report/STATUS.md"

if [ -n "${CI_REPORT_DRY:-}" ]; then
    cat "$report/STATUS.md"
    exit 0
fi
cd "$report"
git init -q
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git add STATUS.md
git commit -qm "$title: $RESULTS"
git push -qf "https://x-access-token:${GH_TOKEN}@github.com/${GITHUB_REPOSITORY}.git" "HEAD:refs/heads/$branch"
echo "reported to $branch"
