#!/usr/bin/env bash
# Run a command in a sandbox that cannot drive the user's desktop.
#
# Two barriers, both structural rather than advisory:
#   1. a private /tmp, so the filesystem X sockets (/tmp/.X11-unix/X0) do not
#      exist inside the sandbox at all;
#   2. no XAUTHORITY and no /run, so the *abstract* unix socket an X server also
#      listens on (@/tmp/.X11-unix/X0) answers "Authorization required" -- the
#      cookie it would need is not mounted.
#
# --unshare-net was tried first and is NOT usable here: Kontakt 8 needs network
# access and exits silently without it (it starts, touches GL, then dies with no
# window and a 2-line log). Measured 2026-10-29. The cookie barrier above is
# what replaces it.
#
#   oracle-sandbox.sh CMD [ARGS...]
#
# SANDBOX_UNSHARE_PID=1 adds a pid namespace. Callers may pass env vars
# (KONTAKT_EXE, ORACLE_*) on the command line.
set -euo pipefail

SANDBOX_DISPLAY="${SANDBOX_DISPLAY:-:78}"
REPO=/tmp/opencode

exec bwrap \
  --dev-bind /dev /dev \
  --proc /proc \
  --ro-bind /usr /usr \
  --ro-bind /opt /opt \
  --ro-bind /etc /etc \
  --symlink usr/bin /bin \
  --symlink usr/sbin /sbin \
  --symlink usr/lib /lib \
  --symlink usr/lib64 /lib64 \
  --bind /home/dracula /home/dracula \
  --tmpfs /tmp \
  --bind "$REPO" "$REPO" \
  --dir /tmp/.X11-unix \
  --chmod 1777 /tmp/.X11-unix \
  ${SANDBOX_UNSHARE_PID:+--unshare-pid} \
  --die-with-parent \
  --new-session \
  --setenv DISPLAY "$SANDBOX_DISPLAY" \
  --unsetenv XAUTHORITY \
  --unsetenv WAYLAND_DISPLAY \
  --unsetenv XDG_SESSION_TYPE \
  -- "$@"
