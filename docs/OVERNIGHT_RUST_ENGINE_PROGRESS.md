# Rust engine continuation — 2026-10-05

Continuation of Pi session `01a108e5-4786-7255-9f9c-82de218c6ec5` in the
isolated `refactor/rust-audio-engine` worktree. The original dirty checkout is
preserved. No public push, release, or local plugin installation has occurred.

Latest validated checkpoint: parser nesting repair `b105706` and the third
mechanical cleanup `1acd310`. The combined v19 source passes 494 scoped
application tests and Rust 1.98 all-target compilation; parser compatibility
passes all 33 tests. The latest full root suite remains v18: 1,544 passed,
zero failed, 35 ignored. Performance prototypes remain uncommitted.

## Integrated base

- Fast-forwarded the verified core/UVI/UI integration through `ce94add`.
- Recovered the five uncommitted latest-MUI renderer follow-up files from Pi.
- Committed current MUI alignment and redundant IME vendor removal as `1dc9467`.
  IME and encoding now use the same locked upstream MUI revision; retained local
  patches still supply renderer owner retirement and native window behavior.
- Parser/framework cleanup is committed as `eb2d1e3`; bit-preserving native
  reference literal cleanup is committed as `91ecb6f`.
- Shared coefficient/control-clock and one-frame convolution work is committed
  as `7b7d683`, supported by 62 passing scoped FX/UVI-effect tests and identified
  release PCM/performance receipts.
- Decoded PCM reuse and pending sparse publication fencing are committed as
  `23c5651`. The new real-WAV race regression fails before the fix and passes
  afterward; the broader v9 engine group passes 253 tests with five ignored.
- Compact mapping layout and UI fixture resource contracts are committed as
  `5db41cb`; all 84 screenshot captures and scoped vector/paging checks pass.
- CLAP terminal identity retention and exact pedal-deferred completion are
  committed as `6ca2d85`; seven bounded lifecycle/PCM/no-heap tests cover internal output-staging
  backpressure, duplicate fencing, reset, capacity and shared source retirement.
- Obsolete positional host-state decoding and selection fields are removed in
  `7d746b7`; current keyed state and native library formats are retained.
- Ignored original script-event retirement and legato owner coverage are
  committed as `91c666c`.
- Precise UVI release velocity and host meter propagation are committed as
  `0c5f68c`, with fixed-packet ownership and Lua boundary checks.
- Proven KSP deferral warnings are committed as `e496748`; both new no-heap
  regressions and all six related v11 checks pass.
- Native sample/resource dependency invalidation is committed as `96544fc`,
  including file and directory alias retargeting checks.
- Private Rust helper cleanup is committed as `82e90a7`; current atomic APIs
  and benchmark ABI fixes as `a815221`.
- Behavior-preserving Rust guards/chunks cleanup is committed as `e90f6b0`;
  v12 filter/generator/modulation/artwork/timing/diagnostics checks pass.
- Truthful worker stage/failure traces are committed as `6d54b4d`, internal
  field/UI helper visibility as `31bc512`, and historical native fixture/unit
  corrections as `e664742`.
- Concurrent header-cache staging is fixed in `ee990d5`, with deterministic
  actual-function failure before the fix and independent header checks afterward.
- The remaining UVI OnePole uses the exact shared coefficient calculation in
  `ac2677e`; its original adapter limits are retained.
- Rust best practices and real-time ownership requirements are committed in
  `AGENTS.md` as `a39b1b7` for every subsequent Rust agent.

## Behavior and performance changes under validation

- Retain terminal-only CLAP note identities until the host accepts `NOTE_END`,
  including duplicate fencing, reset, bounded capacity, and shared-source
  preflight. The corrected shared-source
  fixture passes on v7 with complete PCM, exact ends, and no callback heap use.
- Preserve precise release velocity through fixed UVI packets, projecting to
  MIDI1 only at Lua's input boundary. Transport carries validated time signatures
  and computes bar duration from the current meter.
- Invalidate retained native instruments after preset/sample/resource changes,
  including file and directory sample symlink retargeting or deletion.
- Reuse decoded cache block buffers after successful eviction without evicting
  valid PCM on decode failure.
- Share the exact one-pole coefficient calculation while keeping adapter cutoff
  clamps and mutable state separate. Reset compressor controls on kind changes;
  keep the Phaser's 16-frame clock through fragmentation and automation.
- Optimize construction-known one-frame convolution with cached historical FFT
  output and a short direct prefix. Boundary FFT publication and tail stages are
  unchanged. Independent PCM checks pass. Seven alternating release medians reduce
  busy-case time by 64–83%, ringing tails by 44–66%, and silent cases by 6–18%
  across five IR sizes. This applies to construction-known maximum block 1.
- Reclaim script parents whose note and release are both ignored using the
  existing deferred event retirement path; sounding originals and generated
  children retain ownership. The new sounding-owner regression passes on v7.
- Start import-stage diagnostics before source classification; the failed-load
  regression passes on v7.
- Keep compact Mapping list/inspector columns parallel and captions bounded;
  native paging passes with both sizes visually confirmed.
- Record completion of pedal-deferred release work at its actual resolution
  frame; new within-block NOTE_END coverage passes on v8, with zero callback heap use.
- Remove obsolete positional KONTRA host-state decoding and unused helpers.
  Keyed state and Kontakt/UVI format semantics remain supported.

## Recorded local checks

| Check | Result |
| --- | --- |
| Locked workspace, all-feature test compilation | Passed |
| Focused engine/import/transport/lifecycle tests | 61 passed; corrected shared-source fixture also passes v7 |
| Convolution and native UVI effect fixtures | 4 passed, including maximum block 1 and 64 |
| Repaired native volume, streaming, callback, and editor fixtures | 9 passed |
| Moose core unit suite | 160 passed |
| MUI renderer CPU/unit suite | 45 passed |
| MUI renderer GPU effect smoke suite | 19 passed |
| Moose editor suite | 20 passed |
| Native host headless suite | 21 passed; provider checks run separately |
| Supplied NI parser compatibility suite | 32 passed |
| NCW unit suite | 13 passed |
| NCW decode/write integration suites | 30 passed; benchmark ignored |
| Software provider and X11 visibility checks | 2 passed each |
| No-feature, UVI-only, plugin-without-UVI library compilation | Passed; feature-specific warnings remain |
| Latest root all-feature test compilation | Passed on v16, no warnings; exact tree `7ee810d0f452a7864035336163abf1e428f79a78` |
| Broad v7 application suite | 1,510 passed; 20 failed; 34 ignored |
| Focused v8 corrected fixtures | 16 passed; generated-release fixture corrected and passes v10 |
| Sparse publication regression | Deterministic failure before fix; passed afterward on v9 |
| Focused v9 publication/prefetch/interpolation checks | 4 passed |
| Broader v9 engine group | 253 passed; none failed; 5 ignored |
| Focused v10 budget/UVI/host routing checks | 6 passed; 2 new tests exposed test-recorder allocations |
| Focused v11 budget/UVI/host routing checks | All 8 passed after recorder preparation; callback heap guards remain zero |
| Broad v11 application suite | 1,534 passed; none failed; 34 ignored; tree `3f5ab92ce3fd4cc5d9b3fc7d2eb265780195a65a` |
| Focused v12 style regression groups | 109 passed; 1 ignored; original PCM/order oracles retained |
| Finite alias-pool v12 reproduction | Forwarded release passes; ignored release fails deterministically at note 4,097 |
| Finite alias-pool v13 fix | 3 lifecycle/PCM/no-heap tests pass, including held and waiting owners under pressure |
| Broader v13 engine/KSP/UVI groups | 256 / 120 / 482 passed; none failed; 5 / 1 / 2 ignored |
| Shared sampler prototype v13 | 5 correctness tests pass in debug/release; two original and one narrowed paired benchmark runs pass (36 micro + 288 renderer cases each) |
| Concurrent native header-cache publication | Actual old staging fails deterministically on v15; unique staging passes all six matching cache tests on v16 |
| Identified v11 shipping artifacts | CLI/standalone/plugin built; clean source identity and CLAP/VST3 exports verified |
| Read-only v11 real-library probes | 6 completed renders; finite/nonzero PCM; UVI direct/worker WAV hashes identical |
| UVI mapping paging and mixed MIDI protocols | 3 passed on v5; native mapping also passes v7 |
| One-frame convolution release benchmark | 15 cases passed; paired medians and complete PCM |
| Independent release convolution/cache fixtures | 4 passed |
| Decoded cache release benchmark | Passed; 4.53% median improvement in this run |
| Strict NI parser all-target/all-feature Clippy | Passed before the final archive diagnostic correction |

The full v11 application snapshot passes. Newer source changes have separate
scoped checks; they do not establish universal sound parity. Historical fixture
clock/unit errors and a test-only editor classification barrier were corrected
before that full pass.

The NI parser's 54 file-based tests cannot run without the original corpus under
`tests/data`, `tests/filetype`, `tests/patchdata`, and `test-data`. The checkout
does not provide those trees or retrieval instructions. Supplied synthetic
compatibility tests remain runnable.

A new real-engine reproduction shows finite physical notes whose release
callbacks are ignored exhaust the script event pool after 4,096 completed
notes, despite accepted host retirement. A narrowly guarded pressure-reclaim
fix and held/waiting-owner safeguard pass v13; committed as `b1a1c1a`.
The v11 shipping candidate build is an identified baseline; it will not be
installed with this known defect. Its six bounded library renders completed;
all source and authority metadata remain unchanged. UVI worker telemetry
reported one and six render-deadline misses, so these are offline checks, not
live deadline or native musical-equivalence evidence.

The budget-warning fix emits only proven real deferrals; a stale due timer at
the heap head can hide valid due work behind it. This conservative diagnostic
omission is retained to avoid scanning or pruning up to 8,192 timers after the
audio budget expires. No deadline or instruction limit has been relaxed.

Strict root Clippy still has findings. Exact decimal-to-IEEE754 verification
shows all 3,548 unique excessive-precision suggestions preserve the original
`f32` bits; their guarded cleanup is applied. Strict findings fell from approximately
4,000 to 453, then 412 root library/test findings on v13 (six additional
example findings in that earlier diagnostic index). The v16 cleanup reduces
the reported library/test errors to 240; a normalized JSON index has 239 unique
sites. Normal compilation passes without warnings. The full immutable v16 application
suite finished: 1,542 passed, 35 ignored, one picker completion test failed.
An isolated rerun reproduces the same failure: the worker publishes its answer
before clearing the busy flag. That picker file is unchanged in the cleanup;
a focused ownership/publication repair is being prepared. Remaining findings have not been waived. Authored near-constant `f64`
fixture inputs must retain their original values.

## Next validation

1. Rebuild and validate the latest parallel Rust cleanup against retained PCM,
   lifecycle, UI, and no-heap oracles; preserve a source and binary identity.
2. Check no-feature, UVI-only, plugin-without-UVI, and all-feature configurations.
3. Preserve completed paired release convolution/cache receipts; benchmark new
   performance changes in optimized builds.
4. Build an identified optimized artifact, run the repaired read-only real-library
   probe harness, and install locally with verified backups only after validation.
5. Continue shared-engine and Rust cleanup; preserve explicitly unsupported
   native graphs until an independent signal-order or parameter oracle exists.

Completed release benchmarks use frozen source tree
`3c195d769776a1c4e07e82ba38ec0d22f608549c` in a separate artifact directory.
Convolution binary SHA256:
`acba3d740c3d3c327901a53f14b7854720e2bf0b17b520923ca5119c643aef03`.
The corrected cache benchmark uses frozen tree
`92c58bb4b5e78b54477f61ba4e090bd3c3971a39`, binary SHA256:
`3577ce183d9b0778a0d106a952392200c88ca48bd670149f3abac826f27e0cc1`.
Over 8,704 forced misses, median time is 0.110523s → 0.105513s;
combined allocation/free operations are 79,744 → 62,336. This removes one PCM
allocation and free per miss; codec allocation remains. Background CPU work
was present; these results do not establish hardware deadline guarantees.

The screenshot fixture now reuses one immutable catalog within its run,
preserving all 42 states and both widths. Its v7 broad rerun passes all screenshot states; observed capture intervals
fall from about 30 seconds to about 7 seconds. The repaired full v11 application snapshot passes 1,534 tests with 34 ignored.
Newer alias/sampler work is verified separately on v13; the sampler performance
prototype is not yet committed.

Detailed extraction, logs, and independent review notes are under
`/tmp/kontakto-overnight-handoff/`. The real-library probe harness and plan are
under `/tmp/kontakto-overnight-probes/`.

The sampler benchmark first stopped because its ignored fixture used note ID
zero; playback requires positive IDs. IDs now run from one to 64, and both
paths assert their requested voice counts before timing. Its five ordinary
correctness tests already used valid positive IDs. No sampler performance
claim is made from the aborted run.

The original bulk sampler prototype was measured twice in one optimized binary
against its extracted scalar helper. Nonzero-voice stereo cubic medians improved
7–8%; six-channel linear/cubic medians improved 7–16%, and twelve-channel
linear/cubic medians improved 14–24%. Nearest medians regressed 2–4%, while
mono results were mixed. The first narrowed revision delegates nearest
and mono to the scalar specialization. That narrowed helper benchmark passes
all 36 decode and 288 render cases, but common paths still show small
regressions. The identical-fixture production A/B comparison is complete.
These are helper comparisons with background CPU activity, not original
production-binary A/B measurements or live deadline guarantees. A separate
production harness has been prepared in two frozen source commits before
deciding whether to commit: original `5613e575` and candidate `908a0aef`.
Their only production differences are sampler playback and storage; the
public harness and unrelated repairs are identical. Neither build uses the
unit-test selector. All 294 production pairs preserve complete PCM bits.
However, mono nearest regresses about 10–12%, mono linear 6–10%, stereo
nearest 6–7%, and stereo linear 3–8%. Twelve-channel cubic improves 21–25%.
The helper-delegation candidate is rejected and remains uncommitted.
A second candidate, `586b15cb`, retains the original inline mono, stereo
and nearest bodies and restricts bulk decoding to wider interpolated paths.
Its production build and all 378 pairs of the 54-case comparison pass complete
PCM equality. Six-channel cubic medians improve about 8–13%, twelve-channel
cubic about 19–23%; common mono/stereo medians range from about 5% faster to
3% slower. Further width12 linear/nearest and width6 nearest gates, plus longer
repeats of the six slowest common cases, remain pending before adoption. These
resident Renderer measurements establish neither native DSP equivalence
nor hardware deadline guarantees.

The atomic `try_update` replacement is
[stabilized in Rust 1.95](https://blog.rust-lang.org/2026/04/16/Rust-1.95.0/).
However, the latest locked MUI and UI adapters declare Rust 1.98. The root
minimum is corrected to 1.98 in `fecafd2`. Installed Linux Rust 1.98 now
passes all four production feature configurations and an all-features,
all-targets check against tree `d53a7e20`. Reduced configurations retain
warnings; these compile checks do not prove linking or audio behavior.
The identified audio test and performance binaries use Rust 1.99.

Concurrent native header stores shared a PID-only temporary file: the v15
regression proves the first writer publishes the second writer's source. The
fix follows the existing parsed-cache PID-plus-serial staging pattern. It does
not change entry keys, encoded headers, source stamps, or failure cleanup.
The actual v16 reader verifies each completed publication against independently
opened WAV source headers. This is cache correctness evidence; no load-time
speed claim is made.

The first mechanical cleanup is committed as `cf62444`: 23 files, 375 additions
and 396 deletions. The uncommitted sampler prototype is excluded. The v16
header-cache tests pass all six matching cases, including the deterministic
concurrent publication reproduction; the fix is committed as `ee990d5`.
Validation receipts, external patches and frozen commit references are preserved
under the durable handoff directory as well as `/tmp`.

The picker publication repair is committed as `1c341c0`. Its deterministic
actual Reveal-worker gate fails on the original temporary-lock publication
and passes after retaining the answer mutex through completion bookkeeping.
All five picker tests pass with unchanged cancellation and busy assertions.
The broader immutable v18 suite passes 1,544 tests with zero failures and
35 ignored. Its exact tree is `e32fba00`, executable SHA256 begins
`2d93a6e7dd0cd8a5`. The initial phase overlapped a production example build;
these are correctness results, separate from the quiet paired benchmarks.
The original and narrowed sampler production examples both build and their
complete PCM proofs are bit-identical; the timing rejection is recorded above.

The corrected second mechanical wave passes Rust 1.98 all-targets compilation.
Strict Clippy on the current source still fails, with 267 diagnostic emissions
and 154 unique primary sites, down from v16’s 388 emissions and 239 sites.
The second wave is committed as `f995dd2` after the full v18 runtime receipt.
Remaining argument-count, error-size and ownership findings are not waived or boxed
solely to satisfy lint suggestions.

A separate counted-loop proposal skips decoding unused resident Packed taps
while preserving every physical availability check. Its original optimized
binary passes a genuine NCW-to-NKI-to-Bank fixture with handwritten complete
PCM, exact EOF and physical note-end clocks, zero callback allocation/free,
and zero sparse underruns. The first candidate also passes those gates. Five
alternating pairs improve resident 128-voice integral rendering about 2.0–2.2x,
but one sparse case slows about 5%. That version is held. A narrower five-line
candidate limits skipping to constant integral clocks, actual Packed storage
and resident packets. Separate genuine fractional Packed and ordinary I16/F32
fixtures now supplement the immutable original oracle; their original release
build and both original correctness oracles pass. The five-line candidate
release build and both candidate oracles also pass. Paired measurements and
supplementary sampler gates are underway. No counted-loop optimization has
been committed.

NIS recursive parsing previously completed before the import wrapper's depth
guard. The actual `NIFile::read` boundary test fails on the original parser:
it accepts 65 nested containers after the 64-level exact roundtrip. Repair
`b105706` bounds container depth and, independently, item-data depth to 64;
the post-fix test retains native bytes at 64 and returns structured errors
with offset context at 65. All 33 compatibility tests pass. This is an explicit
input policy for ordinary loader stacks, supported by checked-in shallow
fixtures and stack inspection; it is not an empirical maximum for every
Kontakt library. Manually constructed public trees are outside this guard.

The third mechanical cleanup `1acd310` changes 18 files, 84 additions and 93
deletions. Its combined v19 snapshot (`48f00074`, binary SHA256 beginning
`fc11a0e24fd26a28`) passes 494 scoped tests, including engine/KSP/FX, sampler,
picker/vector/layout, Packed storage, support, preview and scanner checks.
Rust 1.98 all-target compilation passes. Strict Clippy still fails: 230 emissions
and 129 unique primary sites. No floating-point oracle, argument/layout warning
or real-time ownership rule is waived to improve that count.

The next MIDI audit identifies actual CLAP host refusal after internal terminal
staging, plus failed-block output discards. An external native reject-once
regression is prepared; bounded retention and host acknowledgement design
remain under independent review. Anonymous IDs also
require wildcard-aware completion timing across live route changes. These
findings are under review; no unverified terminal retry change is integrated.
