#!/usr/bin/env bash
set -euo pipefail
: "${HOME:?HOME is required}"
rm -f -- "$HOME/.clap/KONTRA.clap" "$HOME/.local/bin/kontakto-standalone" "$HOME/.local/share/kontra/LICENSES.txt"
rm -rf -- "$HOME/.vst3/KONTRA.vst3"
printf '%s\n' 'Removed installed KONTRA files. Libraries, settings and logs remain.'
