#!/usr/bin/env bash
# Write this GitHub Actions run's outcome to a branch, for agents that can't see the Actions page:
#
#   packaging/ci-report.sh BRANCH TITLE
#
# Read it with `git fetch origin BRANCH && git show FETCH_HEAD:STATUS.md`. It lists every finished
# job with its result and, for failed jobs only, the failed steps, the error lines and the log
# leading up to the last error (from GitHub's own job log, so failures in any step show up).
# Needs GH_TOKEN with `contents: write` (to push) and `actions: read` (to read jobs and logs).
# CI_REPORT_DRY=1 prints the report instead of pushing it.
set -euo pipefail

branch="${1:?branch}"
title="${2:?title}"
work="$(mktemp -d)"

# The latest attempt of each job; this report job itself is still running, so it's left out.
gh api "repos/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID/jobs?per_page=100" \
    | jq -c '.jobs[] | select(.status == "completed")' >"$work/jobs"

bad="$(jq -r 'select(.conclusion != "success" and .conclusion != "skipped") | .name' "$work/jobs")"
if [ -z "$bad" ]; then result=success; else result=failure; fi

{
    echo "# $title: $result"
    echo
    echo "- commit: $GITHUB_SHA"
    echo "- run: $GITHUB_SERVER_URL/$GITHUB_REPOSITORY/actions/runs/$GITHUB_RUN_ID (attempt $GITHUB_RUN_ATTEMPT)"
    echo "- finished: $(date -u '+%Y-%m-%d %H:%M UTC')"
    [ -n "$bad" ] && echo "- failed jobs: $(echo "$bad" | paste -sd, - | sed 's/,/, /g')"
    echo
    echo "## Jobs"
    echo
    jq -r '"- \(.name): **\(.conclusion)**"
        + (if .conclusion == "success" or .conclusion == "skipped" then ""
           else ([.steps[] | select(.conclusion == "failure") | .name] as $s
                 | if ($s | length) > 0 then " (failed step: \($s | join(", ")))" else "" end) end)' \
        "$work/jobs"

    jq -r 'select(.conclusion != "success" and .conclusion != "skipped") | "\(.id)\t\(.name)"' "$work/jobs" \
        | while IFS=$'\t' read -r id name; do
            echo
            echo "## $name"
            echo
            if ! gh api "repos/$GITHUB_REPOSITORY/actions/jobs/$id/logs" >"$work/raw" 2>/dev/null; then
                echo "(log unavailable)"
                continue
            fi
            # Drop the timestamp GitHub puts on every line.
            sed -E 's/^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9:.]+Z ?//' "$work/raw" >"$work/log"
            last="$(grep -n '##\[error\]' "$work/log" | tail -n 1 | cut -d: -f1 || true)"
            if [ -n "$last" ]; then
                echo "Errors:"
                echo '```'
                grep '##\[error\]' "$work/log" | head -n 40
                echo '```'
                echo "Log up to the last error:"
                echo '```'
                head -n "$last" "$work/log" | tail -n 80
            else
                echo "End of the log (no error lines; the job may have timed out or been cancelled):"
                echo '```'
                tail -n 80 "$work/log"
            fi
            echo '```'
        done
} >"$work/STATUS.md"

if [ -n "${CI_REPORT_DRY:-}" ]; then
    cat "$work/STATUS.md"
    exit 0
fi
mkdir "$work/repo"
cp "$work/STATUS.md" "$work/repo/"
cd "$work/repo"
git init -q
git config user.name "github-actions[bot]"
git config user.email "41898282+github-actions[bot]@users.noreply.github.com"
git add STATUS.md
git commit -qm "$title: $result"
git push -qf "https://x-access-token:${GH_TOKEN}@github.com/${GITHUB_REPOSITORY}.git" "HEAD:refs/heads/$branch"
echo "reported to $branch"
