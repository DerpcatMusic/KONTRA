# W13 standalone crash delivery and capture

Status: **source-only READY for coordinator review**. The coordinator explicitly requested commit/push without heavy jobs on 2026-10-09; machine validation is deferred until after W14. No cargo build, runtime test, crash delivery, install or timing has run for this change. READY here describes the completed source, not a green runtime verdict.

Tester evidence is row 3 of `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/tester-logs-NwxC/TRIAGE.md`: three delivery-worker failures with `dlopen refused kontakto-standalone`, plus nine unclean reports with no native stack or signal. These are two distinct gaps. The shared worker pinning fallback resolves a relative `dladdr` filename against cwd; Linux executable identification now compares loaded image bases using kernel `AT_PHDR`. Plugins still take a permanent loader reference before detached work. v1 `0cb7a8a0:src/support/crash.rs` has the same faulty fallback, so this is not a v1 code port.

Only the standalone entry point starts owned panic/native capture. The device-default preflight remains before worker startup and environment mutation. A main-loop Rust panic records its backtrace and exits unsuccessfully with the crash marker retained. Linux fatal handlers write the original signal, signal code, fault address, actual fault instruction and at most 64 frame addresses to pre-opened private files. Frame reads use kernel-checked `process_vm_readv`; the handler does not allocate, lock, symbolize or invoke libc `backtrace()`. Bounded current image mappings cover GPU/audio libraries loaded after startup. Fatal disposition is restored and the same signal re-raised. SIGKILL and normal termination remain unconfirmed without external evidence.

Recovery checks session identity, PID, start time, executable and signal time against the recorded session. Native evidence joins the existing incident and delivery path. Exact JSON/mapping originals remain private and are included only in manual export; the existing sanitized automatic transport and acknowledgement policy remain in force. The plugin entry points install no process-wide hooks.

Machine-turn validation plan (all cargo through `kontakto-heavy`, no quiet request during builds):

- At `d53e772d`, run `support::crash::tests::standalone_delivery_worker_survives_bare_argv0_and_changed_directory` for the loader red witness.
- At `311ac1ee`, run `support::standalone::tests::standalone_fault_and_panic_are_recovered_without_coredumpctl` for the missing-native-file red witness. This checkpoint intentionally has a no-op native installer.
- On the final source, run both regressions, `frame_addresses_require_executable_mappings`, repeated own/shared-library pinning, and detached unload ownership. The native fixture disables core dumps, removes `coredumpctl` from PATH, creates actual SIGILL/SIGSEGV fault instructions, checks top-frame identity and exit signals, recovers native/panic incidents, keeps SIGKILL unconfirmed, rejects mismatched owners, and compares manual-export originals byte-for-byte.
- On the assigned machine turn, run root `cargo test --profile ci --features standalone --no-run`; this also compiles the changed standalone entry point. The coordinator’s source-only push instruction supersedes the usual pre-push no-run requirement for this handoff.

Limit: raw native frame walking can stop at the fault instruction when optimized code omits frame pointers or the chain is corrupt. The alternate signal stack covers the startup/main thread; other threads use their own existing stack. Linux x86_64 is the tester platform; AArch64 context extraction is source-only and unvalidated. No retrospective backtrace is claimed for older reports, and no live upload has been sent. GNU libc documents `backtrace()` as unsafe inside asynchronous signal handlers: <https://www.sourceware.org/glibc/manual/latest/html_node/Backtraces.html>. Executable program-header identity: <https://man7.org/linux/man-pages/man3/getauxval.3.html>.

Suggested release note, only after green: “Standalone crash reports now start reliably and retain native signal and backtrace evidence.”

Source review: `git diff --check` passes, and the native module passes `rustfmt --edition 2024 --check src/support/standalone.rs`. Both failing-first source checkpoints are retained; their runtime red and final green outcomes remain pending.

NEXT: after W14, run machine-turn crash red→green and standalone root no-run; then UI validation and the assigned frozen CLAP cells, with handoff per the coordinator’s latest queue.
