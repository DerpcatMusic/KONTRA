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
"$directory/actionlint" -shellcheck=
