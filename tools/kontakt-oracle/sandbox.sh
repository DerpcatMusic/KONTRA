#!/usr/bin/env bash
# Run a command in a sandbox that cannot reach the user's X displays.
#
# The guarantee is structural, not advisory:
#   * a private /tmp, so the filesystem X sockets (/tmp/.X11-unix/X0) are absent;
#   * --unshare-net, because X servers also listen on an *abstract* unix socket
#     (@/tmp/.X11-unix/X0) which ignores mount namespaces -- without a network
#     namespace a sandboxed client can still reach :0.
# DISPLAY is set to the sandbox display, but even a tool that overrides DISPLAY
# has nothing to connect to. There is no XAUTHORITY in the environment and no
# wayland socket, so no input can reach the real desktop.
#
#   oracle-sandbox.sh CMD [ARGS...]
#
# Callers may still pass env vars (KONTAKT_EXE, ORACLE_*) on the command line.
set -euo pipefail

SANDBOX_DISPLAY="${SANDBOX_DISPLAY:-:78}"
REPO=/tmp/opencode

exec bwrap \
  --dev-bind /dev /dev \
  --proc /proc \
  --ro-bind /usr /usr \
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
  --unshare-pid \
  --unshare-net \
  --die-with-parent \
  --new-session \
  --setenv DISPLAY "$SANDBOX_DISPLAY" \
  --unsetenv XAUTHORITY \
  --unsetenv WAYLAND_DISPLAY \
  --unsetenv XDG_SESSION_TYPE \
  -- "$@"
