#!/usr/bin/env bash
# pid of the wineserver serving ~/.wine (the one Kontakt uses); other prefixes' servers (WINEPREFIX set to elsewhere) are ignored
[ -n "${KONTRA_WS_PID:-}" ] && { echo "$KONTRA_WS_PID"; exit 0; }   # a session script pins the persistent wineserver it started
for p in $(pgrep -x wineserver); do
  e=$(tr '\0' '\n' </proc/$p/environ 2>/dev/null | sed -n 's/^WINEPREFIX=//p')
  [ -z "$e" ] || [ "$e" = "$HOME/.wine" ] && { echo $p; exit 0; }
done
