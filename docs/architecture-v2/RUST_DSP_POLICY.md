# Rust and realtime DSP review policy

Rust Doctor 0.7.0 is the pinned static-analysis prerequisite for new work.
It supplements the independent core's correctness, allocation/free and render
checks. It cannot certify realtime safety or make a sampler production-ready.
No catalog rule is disabled by `rust-doctor.toml`.

## Running and interpreting the gate

```sh
CARGO_TARGET_DIR="$PWD/target" npx -y rust-doctor@0.7.0 . --yes --scope full --json > rust-doctor.json
python3 .github/scripts/check_rust_doctor.py rust-doctor.json
```

The workflow installs the repository's Rust 1.99.0 toolchain and Linux build
dependencies. PRs and merge groups compare findings against the exact event base
commit; main pushes and manual runs scan the full workspace. Actions are pinned
by commit, Rust Doctor by version, credentials are not persisted, and the job
has read-only repository permissions. JSON evidence is uploaded even on failure.
An incomplete report, missing report, malformed JSON or failed diagnostic gate
fails CI. A process failure is also preserved independently of the JSON gate.
No score threshold is used.

Security and correctness findings are errors, as are leaked ownership via
`mem_forget`, unreaped child processes and missing unsafe API safety docs.
Remaining rules retain their warning severity and remain review obligations.
This is deliberately not an all-warnings gate across the inherited codebase.
A PR baseline is not permission to reproduce old defects in the new core.
Main's full gate currently fails on existing debt; do not label it green or
silence those findings to get a better number.

Rust Doctor runs its own workspace Clippy pass with a curated lint set. It does
not replace ordinary Clippy, all-target tests, feature/platform builds, MSRV
validation or the shipping-profile CI. The workflow additionally runs strict
all-target Clippy and release tests for `sampler-core` and `sampler-native`.

For machines with a full `/tmp`, use an existing writable `TMPDIR` outside the
repository when running baseline scope. Version 0.7.0 rejects baseline temporary
storage inside the repository. Set `RUSTC_WRAPPER=` if a shared sccache daemon is
still using an unavailable temporary directory. Do not delete unrelated caches.

## Applying Rust rules to DSP

| Finding or recommendation | Project decision and reason |
| --- | --- |
| Unsafe `Send`/`Sync`, guards held across await, recursive mutex locking | Block new findings. Thread-safety violations are correctness problems. A suggested `Arc<Mutex<_>>` is not an acceptable audio-thread repair: separate owners and transfer bounded messages. |
| `unwrap`, `expect`, indexing, `unreachable`, assertions | Untrusted indices and handles must return typed errors or documented bounded fallbacks. Private validated indices may use safe indexing with a local invariant and tests. No `get_unchecked` to silence warnings; no fabricated default that hides corrupted ownership. Existing invariant unwraps are review debt, not a general exception. |
| `Arc`, `Rc`, ownership and redundant allocations | Prefer explicit owners, borrowed immutable slices and generational handles. Refcounting alone does not guarantee realtime safety: the last strong reference can destroy/deallocate its payload. Retire owned assets off the callback. Do not add shared ownership just to satisfy an idiom. |
| Slices rather than `&Vec`, copying slices, unnecessary clones | Normally accept. Verify identical aliasing/lifetime semantics and output. Heap preparation belongs off the callback; replacing a small copy with refcount traffic is not automatically faster. |
| Primitive unstable sorting | Fine where equal elements are indistinguishable. Event queues require deterministic equal-time ordering; retain insertion order or a total timestamp/sequence ordering. No callback sort that allocates. |
| `println`, `eprintln`, logging | Forbidden in callback paths, including error recovery. Bounded counters/telemetry cross to a non-realtime consumer. Native CLI progress/errors are valid terminal I/O; replacing them with a logging dependency does not improve DSP. Keep the warning visible at those entry points. |
| Complexity, duplicated bodies, file length, argument count | Review responsibility boundaries. Do not split hot loops or create generic frameworks solely for a score. Similar syntax is not identical semantics, especially across independently owned pools. Complex parsers need malformed-input tests; numerical kernels need reference-output and performance evidence. |
| Release overflow checks | Counters, generations, offsets and sizes need explicit checked arithmetic; intentional modular arithmetic uses wrapping operations. Enabling panic-on-overflow is not a recovery policy for an audio callback. Keep the warning visible, test overflow boundaries and measure shipping code before changing the global profile. Never globally disable checks to chase throughput. |
| Float comparisons, numeric conversions, fast math | Follow signal semantics. Exact integer-derived boundaries/zero sentinels differ from approximate numerical comparisons. Validate finite/range constraints at admission, avoid unchecked truncation of protocol precision, and test NaN/Inf/denormals. Changes to precision, reassociation or SIMD require quality and timing evidence. |
| Unused dependencies, duplicate versions, orphan modules | Inspect feature gates, macros, build scripts and target-specific linkage before deleting. New core currently needs no third-party dependency. Keep native detectors enabled, but their source-text evidence is not a compiler proof. |
| Broad lint exemptions | No blanket crate/module exemptions for convenience. An unavoidable allowance belongs on the smallest item with a `reason` and evidence. The allocation-counting test's unsafe allowance is now scoped to its forwarding allocator implementation. Production core continues to forbid unsafe. |

Rust API practice here means typed identities, private ownership state, meaningful
errors, validated constructors, explicit failure/panic contracts and predictable
destruction. Public abstraction, genericity or serialization is added for a real
consumer, not because a checklist names it. Caller-provided output buffers are
intentional: returning a freshly allocated audio buffer would violate the DSP
contract despite general API guidance preferring return values to out parameters.

## Mandatory realtime evidence beyond static analysis

- Preparation may allocate; callback execution, overload, cancellation, reset and
  retirement must allocate **and free** zero heap objects on the calling thread.
- No locks, waits, joins, filesystem/network/UI calls, blocking logging, lazy
  initialization or unbounded retries in callback-reachable paths. Audit callees
  and destructors, not just `render`. The allocator test does not prove these.
- Every pool/queue/continuation has a finite capacity, explicit exhaustion policy
  and bounded execution. Avoid retrying full queues inside the callback. Preserve
  terminal events under backpressure and prevent stale generations from aliasing.
- Test sample-time behavior across zero, irregular and standard block partitions,
  same-frame ordering, saturation and repeated shutdown. Validate malformed host
  data and prepared assets before they can violate internal invariants.
- Measure release callback distributions and deadline misses at stated workloads
  and hardware, plus cold storage/overload when those paths exist. Keep numerical
  reference comparisons, aliasing/loop/modulation tests and finite-output checks.
  A fast resident-PCM benchmark does not establish resampler quality or streaming
  performance. No competitor superiority claim follows from a lint score.

## Initial scan evidence — 2026-10-05

Source baseline: `e6a420f` on `docs/plan-v2-architecture`, Rust/Clippy 1.99.0,
Rust Doctor 0.7.0, default workspace features, Linux. Local raw reports live in
ignored `artifacts/architecture-v2/rust-doctor/`; CI retains future reports as
artifacts. These are source-tree scan counts, not measured audio defects.

- Initial attempt was incomplete: shared sccache could not write to full `/tmp`.
  Its result is retained as `initial.json`, never accepted as a clean scan.
- Retried full default-policy scan: `complete=true`, no execution errors,
  **4,721 warnings**, 59 informational diagnostics, 4,780 distinct diagnostics.
  Dominant categories by rule: 2,650 indexing, 1,105 stdout, 337 unwrap,
  215 complex-function and 126 oversized-unit findings.
- Configured full scan before the narrow test-allocator cleanup: `complete=true`,
  **34 errors, 4,687 warnings, 59 informational diagnostics**. Gate fails as
  intended. Errors concern unpinned git dependencies, old/vendor panic/placeholders
  and a vendor `Send` implementation; they require contextual review before porting.
- The two new crates account for **76 findings** in that pre-cleanup scan: 32
  unwrap, 32 indexing, three expect, three complex-function and six other findings.
  There are no error-severity findings in those crates under this policy.
- The tool reports scores 28 (default) / 27 (configured), both explicitly
  `authoritative=false`, even though execution completed. Preserve that flag;
  neither number is a quality verdict. Completeness and diagnostic evidence are
  what this gate checks.

Validation: workflow actionlint and all eight workflow safety tests pass. Strict
Clippy and release tests pass for both new crates (17 unit, three realtime, one
WAV test). The final complete baseline scan against `e6a420f` passes: zero introduced
findings and two fixed by the allocator cleanup (74 new-crate findings remain). Hosted GitHub execution has not
been run from this local checkout.

Next review before expanding callback functionality: prove or replace the new
core's private invariant unwrap/index sites; review the WAV parser's validated
ranges and simplify only where the format contract becomes clearer. The old
core's thousands of findings are an inventory for replacement, not instructions
to import its ownership model.

## Sources and authority

- [Requested Rust Doctor README](https://docs.rs/crate/rust-doctor/0.7.0/source/README.md)
- [Configuration](https://rust-doctor.com/docs/configuration),
  [CI integration](https://rust-doctor.com/docs/ci-cd),
  [rule catalog](https://rust-doctor.com/rules),
  [analysis limitations](https://rust-doctor.com/docs/limitations)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html)
- [Rust Arc ownership/destruction](https://doc.rust-lang.org/std/sync/struct.Arc.html)
- [Cargo overflow and panic profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [JACK realtime process callback requirements](https://jackaudio.org/api/group__ClientCallbacks.html)
- Repository [architecture and validation contract](PLAN.md#validation-and-performance)

The DSP decisions above are this project's policy, derived from these sources and
its ownership contract; they are not additional checks claimed to exist in Rust
Doctor. No separately named DSP/Rust skill was supplied or found in this session.
