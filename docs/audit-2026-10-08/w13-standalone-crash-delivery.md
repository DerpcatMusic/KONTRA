# W13 standalone crash delivery and capture

Status: **runtime GREEN / READY** on Linux x86_64. Both intended baseline failures reproduced, five focused checks passed, and root `cargo test --profile ci --features standalone --no-run` passed. Every cargo call ran sequentially through `kontakto-heavy`; no install, timing or live report upload was performed. Validated source is `1a5f68df82baad1d9a725e189e02e2a3bf874c7e`; its production code is identical to `3baa6a40`.

Tester evidence is row 3 of `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/tester-logs-NwxC/TRIAGE.md`: three delivery-worker failures with `dlopen refused kontakto-standalone`, plus nine unclean reports with no native stack or signal. These are two distinct gaps. The shared worker pinning fallback resolves a relative `dladdr` filename against cwd; Linux executable identification now compares loaded image bases using kernel `AT_PHDR`. Plugins still take a permanent loader reference before detached work. v1 `0cb7a8a0:src/support/crash.rs` has the same faulty fallback, so this is not a v1 code port.

Only the standalone entry point starts owned panic/native capture. The device-default preflight remains before worker startup and environment mutation. A main-loop Rust panic records its backtrace and exits unsuccessfully with the crash marker retained. Linux fatal handlers write the original signal, signal code, fault address, actual fault instruction and at most 64 frame addresses to pre-opened private files. Frame reads use kernel-checked `process_vm_readv`; the handler does not allocate, lock, symbolize or invoke libc `backtrace()`. Bounded current image mappings cover GPU/audio libraries loaded after startup. Fatal disposition is restored and the same signal re-raised. SIGKILL and normal termination remain unconfirmed without external evidence.

Recovery checks session identity, PID, start time, executable and signal time against the recorded session. Native evidence joins the existing incident and delivery path. Exact JSON/mapping originals remain private and are included only in manual export; the existing sanitized automatic transport and acknowledgement policy remain in force. The plugin entry points install no process-wide hooks.

Validation receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w13-standalone-crash-runtime-20261009/{RESULT.json,delivery-renderer-check/RESULT.json,RUNTIME-READY.json,SHA256SUMS}`. The original failed run remains intact.

| Check | Source | Result |
|---|---|---|
| Bare argv0 plus changed cwd | `d53e772d` | RED: `dlopen refused kontakto-standalone` |
| Native signal evidence with no native installer | `311ac1ee` | RED: `native signal evidence absent` |
| Real SIGILL/SIGSEGV, panic, recovery and export | `1a5f68df` | PASS 1/1 |
| Bare argv0 plus changed cwd | `1a5f68df` | PASS 1/1 |
| Executable-map frame validation | `1a5f68df` | PASS 1/1 |
| Repeated executable/shared-library pinning | `1a5f68df` | PASS 1/1 |
| Unload with blocked detached worker | `1a5f68df` | PASS 1/1 |
| Root standalone no-run | `1a5f68df` | PASS, including `src/standalone.rs` |

The first final-source native run failed because the test checked `preview_diagnostics()`, which intentionally omits correlated platform evidence. Test-only `1a5f68df` checks `render_complete_diagnostics()`, the renderer used for report delivery. No production behavior or assertion about captured evidence was weakened.

The native fixture disables core dumps, removes `coredumpctl` from PATH, creates actual SIGILL/SIGSEGV fault instructions, checks top-frame identity and exit signals, recovers native/panic incidents, keeps SIGKILL unconfirmed, rejects mismatched owners, and compares manual-export originals byte-for-byte. Panic evidence contains real Rust frames. No signal evidence is invented for SIGKILL.

Limit: raw native frame walking can stop at the fault instruction when optimized code omits frame pointers or the chain is corrupt. The alternate signal stack covers the startup/main thread; other threads use their own existing stack. Linux x86_64 is the tester platform; AArch64 context extraction is source-only and unvalidated. No retrospective backtrace is claimed for older reports, and no live upload has been sent. GNU libc documents `backtrace()` as unsafe inside asynchronous signal handlers: <https://www.sourceware.org/glibc/manual/latest/html_node/Backtraces.html>. Executable program-header identity: <https://man7.org/linux/man-pages/man3/getauxval.3.html>.

Suggested release note, only after green: “Standalone crash reports now start reliably and retain native signal and backtrace evidence.”

Source review: `git diff --check` and `rustfmt --edition 2024 --check src/support/standalone.rs` pass. Merge the complete `ce714f75..HEAD` branch stack; `3baa6a40` alone is only the earlier source-review receipt. Existing compiler warnings remain.

NEXT: W0 integrates the crash stack; W13 runs assigned frozen CLAP cells and hands directly to W11. Report/Settings UI remains on its separate source-only HOLD branch until validation.
