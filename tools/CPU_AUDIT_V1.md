# Pinned v1 CPU adapter

Product source and Cargo.lock are exactly v1 `0cb7a8a0`. The example adapts the original CPU audit engine seam and copies the CLI bench-stream/load path for equivalence validation. It writes metadata only; decoded PCM stays in RAM. The note/block sequence is copied from v2 `73e6089b:tools/cpu-audit-common.rs`.

Build through `~/.cache/kontakto-heavy env CARGO_PROFILE_RELEASE_STRIP=none CARGO_PROFILE_RELEASE_DEBUG=line-tables-only cargo build --locked --release --example cpu_audit_v1 --no-default-features --features library-access`. No target-cpu override is used. Keep parsed-instrument cache disabled with `XDG_CACHE_HOME=/dev/null`.

Audit: `cpu_audit_v1 ORIGINAL_NKI 32|64|256 piano|strings|fx`.
Equivalence: `cpu_audit_v1 bench-stream ORIGINAL_NKI NOTES SECONDS`; compare with the untouched frozen CLI using the same arguments.
Profiles use CPU_AUDIT_READY/CPU_AUDIT_FINISHED markers around the paced sequence, excluding load and teardown. Leaf samples are estimates of exclusive CPU cost, not exact stage timers or proof of p99 attribution.

Frozen path: `~/.cache/kontra-scan/cpu-v1/bin/cpu-audit-v1`; provenance is in BUILD.json next to it. This adapter does not implement product changes or replace anything under `~/.cache/kontra-v1`.
