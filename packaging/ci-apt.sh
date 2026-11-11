#!/usr/bin/env bash
# Install Ubuntu packages on a GitHub Actions runner:
#
#   packaging/ci-apt.sh PACKAGE…
#
# Tries up to three times, a minute apart. Now and then a runner can't reach Ubuntu's mirrors for a
# few minutes (it can't even resolve their names); that shouldn't fail a CI run or a release.
set -uo pipefail

for attempt in 1 2 3; do
    if sudo apt-get -q -o Acquire::Retries=3 update && sudo apt-get -q -y -o Acquire::Retries=3 install "$@"; then
        exit 0
    fi
    if [ "$attempt" -lt 3 ]; then
        echo "::warning::apt-get failed (attempt $attempt of 3); trying again in a minute"
        sleep 60
    fi
done
echo "::error::apt-get couldn't install $* (3 attempts)"
exit 1
