# Rust and realtime DSP review policy

Rust Doctor 0.7.0 is the pinned static-analysis prerequisite for new work.
It supplements the independent core's correctness, allocation/free and render
checks. It cannot certify realtime safety or make a sampler production-ready.
No catalog rule is disabled by `rust-doctor.toml`.

## Running and interpreting the audit

Use Bun and the pinned version from the existing worktree:

```sh
bunx --package rust-doctor@0.7.0 rust-doctor . --yes --scope full --json > "$AUDIT_DIR/rust-doctor.json"
```

Choose `AUDIT_DIR` on the dev drive. Keep Bun's install cache, `TMPDIR`, and
`XDG_CACHE_HOME` on that drive as well. `CARGO_BUILD_JOBS=2` bounds compilation;
`CARGO_NET_OFFLINE=true` uses already available dependencies. Retain stderr and
the process exit code independently of the JSON report. Run it in a serialized
Cargo slot and outside benchmark timing windows. Respect the shared Cargo target
configuration: do not override the target directory or copy the repository to
scan/build it. Full scope scans the actual workspace, including its inherited
and vendor debt. Identify first-party findings by their real source paths.

The configuration keeps every catalog rule enabled. Security and correctness
findings are errors, as are `mem_forget`, unreaped child processes and missing
unsafe API safety docs. Other rules retain their warning severity and remain
review obligations. No score threshold replaces a finding review. A malformed,
incomplete or failed scan is not a clean result; preserve its producer failures
and authoritative-score flag. Keep historical debt visible while distinguishing
new regressions from existing findings.

Rust Doctor runs a curated workspace Clippy pass over libraries and binaries.
Run ordinary all-target Clippy and relevant tests separately, using the real
first-party package names and feature sets. A scan is not evidence that shots,
scan-only, alternate platform or library-access-off code was checked. Do not
silence recovered DSP constants or change floating-point expressions merely to
make a lint pass. Any justified allowance belongs on the smallest item, with a
reason and native/reference evidence.

The older workflow, `check_rust_doctor.py`, projected-core runner and its
configuration referenced by previous versions of this document are absent from
this branch. Its historical scan results below are retained as historical
receipts, not evidence of a currently running CI gate. Restore automation against
the real workspace if needed; do not resurrect the copied-workspace build path.
The earlier user-selected new-core target was 90/100. That historical scoped
score cannot be transferred to a different source revision or a full workspace
report, nor recreated by filtering findings and calling the result authoritative.

## Current setup — 2026-10-15

Rust Doctor 0.7.0 was fetched with Bun into the dev-drive audit cache. Its version
and compiled rule catalog were checked; the configuration names real rules and
disables none. The full current-worktree scan ran at `a7ea03b56` with 1,168 tracked source
hashes unchanged before/after. Its workspace Clippy command completed with exit
0 and `build_finished=true`. The process returned 1 because the overall report
is **incomplete**: its source resolver reported 35 unresolved module declarations,
including real `#[path = "..."]` modules accepted by the compiler. Do not rewrite
valid module ownership or disable source rules to hide this analyzer limitation.

The retained report contains 93 distinct error-level findings, 11,198 warnings
and 81 informational findings. Of the errors, 57 are first-party sites: 50
`unreachable!` branches, three unpinned Git dependency declarations, three
`panic_in_result_fn` sites and one `mem_forget` site. These categories require
contextual review; a private invariant branch is not proof of an observed crash.
The other findings include vendor/dependency debt. The emitted score has
`authoritative=false`, and the gate is `not-evaluated`; no score or green audit
is claimed. Group counts are the report's distinct diagnostic counts, not its
larger occurrence totals.

Private source/tool/command identities and the complete report are retained in
`PluginArtifacts/kontra-rust-audio-audit-20261015/`, including
`scan-receipt.json` and `first-party-errors.json`. The full report SHA-256 is
`6eca759b3623a5391471725c99815b1d3479d010493909ec97b2144e9cbc34aa`.
Toolchain: Rust/Clippy 1.99.0. No rule was disabled for this run.

The executable audio checks already include callback allocation **and free**
counting, streaming/residency budgets and lifetime tests, source/modulation PCM
oracles, irregular block partitions, callback benchmarks, and native CLAP/VST3
host lifecycle checks. They remain required alongside this static audit. Static
findings do not establish absence of locks, disk reads, deadline misses or audio
artifacts on every callback path.

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
