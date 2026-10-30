#!/usr/bin/env bash
set -euo pipefail
: "${HOME:?HOME is required}"
rm -f -- "$HOME/.clap/KONTRA.clap"
rm -rf -- "$HOME/.vst3/KONTRA.vst3"
printf '%s\n' 'Removed installed KONTRA files. Libraries, settings and logs remain.'
