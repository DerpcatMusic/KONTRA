The crash recovery, native OS collectors, redactor, report summary limits and acknowledgement protocol are adapted from owner-maintained BUFFR commit fd2fdba92f3f71cee24c0c72a39aa190fc9ee414:

- crates/buffr-engine/src/support/crash.rs
- crates/buffr-engine/src/support/platform.rs
- crates/buffr-engine/src/support/report.rs
- crates/buffr-engine/src/support.rs
- vendor/derpcat-flight-recorder
- crates/buffr-durable-file

BUFFR declares ISC in its workspace manifest. The ISC notice is retained in LICENSE-BUFFR and in both vendored crates; the recorder retains its original license and README. These files remain ISC; KONTRA's repository license does not replace those notices.

KONTRA differences: its existing build identity and diagnostics worker supply the metadata and journal events; proprietary source excerpts remain local; numbered UTF-8 chunks preserve events beyond the recorder's detail limit; previous-session reports retain full evidence until a matching SHA-256 receipt. The initial marker is written before system metadata subprocesses. Legacy macOS .crash files are also recognized by exact PID. Failed submissions persist a local receipt/error and become eligible for retry on a subsequent load, including delayed OS evidence. BUFFR's host-wide Rust panic hook and abort-on-cleanup-failure are deliberately omitted in the plugin; no native crash handler is installed. Native stacks therefore depend on accessible OS/host artifacts. Captured panic markers remain readable for recovery, but this integration does not invent a process-global capture hook.
