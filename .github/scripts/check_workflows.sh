#!/usr/bin/env bash
# Download the official, version-pinned actionlint binary and verify its checksum.
set -euo pipefail
version=1.7.12
archive="actionlint_${version}_linux_amd64.tar.gz"
directory=$(mktemp -d)
trap 'rm -rf "$directory"' EXIT
curl --fail --silent --show-error --location --retry 3 \
  "https://github.com/rhysd/actionlint/releases/download/v${version}/$archive" \
  --output "$directory/$archive"
printf '%s  %s\n' '8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8' "$directory/$archive" | sha256sum --check
tar -xzf "$directory/$archive" -C "$directory" actionlint
# Shellcheck is checked separately when deliberately introduced; do not depend
# on whether a particular runner image happens to preinstall it.
# ponytail: actionlint 1.7.12 predates concurrency.queue; remove this exception
# when its pinned release supports the GitHub-documented FIFO queue.
python3 - <<'PYQUEUE'
from pathlib import Path
import re
for workflow in Path('.github/workflows').glob('*.yml'):
    lines = workflow.read_text().splitlines()
    for i, line in enumerate(lines):
        if re.match(r'\s*queue:', line):
            assert workflow.name == 'nightly.yml'
            assert lines[i-2:i+1] == [
                '      group: nightly-publish',
                '      cancel-in-progress: false',
                '      queue: max'], 'Only the validated publication queue is supported'
PYQUEUE
"$directory/actionlint" -shellcheck= -ignore '^unexpected key "queue" for "concurrency" section\. expected one of "cancel-in-progress", "group"$'
