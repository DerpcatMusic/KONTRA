# Doctor remediation checkpoint

The complete same-policy corrected producer passes its error gate at
`c41e24dc1`: zero error findings, zero producer errors, 11,475 warnings and
85 informational diagnostics. No rule or severity was waived. This is an
error-gate result, not a warning-free audit, final combined validation or release
acceptance.

The historical baseline failed with 69 error-level findings. Subsequent complete
reports recorded 11, six, three and finally one before this zero-error result.
These 69 removed source patterns were not 69 observed crashes. Findings were
separated into malformed-input defects, private validated invariants and
analyzer interpretation limits before their ownership or refusal boundaries
changed. The original failing reports remain retained.

## Executable evidence

- The same public NI reader tests reproduced seven baseline panics and one
  incorrect tag acceptance, with three positive controls. All eleven pass
  after typed reader corrections. The vendor library separately passes 65
  tests with 53 retained data/manual ignores.
- Five identical renderer owner-refusal tests fail on the explicitly
  instrumented baseline and pass with the owner repair. That baseline has a
  test-only adapter and is not presented as an untouched binary.
- A real GPU test passed on an AMD Radeon RX 6600 using RADV/Vulkan. One
  persistent renderer handled eight expected typed ownership refusals and
  five recovery/reuse frames. Fourteen full-image readbacks compared 28,672
  pixels against independent literal RGBA expectations, including preserved
  target contents after refusal and recovery from a refused atlas resize.
  Device acquisition, readback, worker execution and outer process execution
  have explicit deadlines; unavailable hardware is a failure, not a passing
  skip.
- First-party all-target Clippy passes with shots and standalone features.
  The earlier full test matrix failed two targets: an authored KSP regression
  fixture used an already-evaluated callback, and generated UVI ledger source
  locations were stale. Both were corrected and their focused tests pass.
  Those focused results do not retroactively make the earlier full-matrix
  receipt green. A final coherent matrix and candidate native/editor gates
  remain required.

The next typed Cascade and dense-SVF ownership correction passed all-target
Clippy, 551 core library tests (11 existing ignores), and 44 focused runtime
tests. Native-vector expectations, numerical equations and goldens were
unchanged. The runtime tests include callback allocation/deallocation checks,
addressed filter targets, expression generations, independent bus/voice
registries, trace equivalence and parallel runtime behavior. Initial compiler
failures from missed internal caller migrations and an authored fixture's
Box-slice argument were retained and corrected before this passing run.

The subsequent typed batch-owner change passed all-target Clippy, 556 core
library tests (11 existing ignores), and 48 focused runtime tests. Scalar and
batch chains retain one operation owner; scalar views borrow the same payloads.
The runtime tests cover exact f64 trace values, actual parallel rendering,
shifted batch indices across retained plans, voice reuse, and callback
allocation/deallocation. Existing native-vector expectations remain unchanged.
Independent review found one missed count-field caller migration, which was
corrected before this passing run; no additional actionable source defect was
found. The failed first compilation remains in the receipts.

Compiler layout measurements include 2,576-byte scalar operations, 1,712-byte
batch operations, and 96-byte parallel runs versus the prior 72-byte field
layout. These figures are not a measured whole-plan memory saving or callback
timing result. Final combined performance and native/editor gates remain open.

## Corrected control review and final Kontakt ownership

The initial control-store candidate passed existing checks but independent
review found a lost ordinary non-Real WavetableForm refusal and an ineffective
loop-remapping test. Both findings were addressed in a separately frozen patch
and a new source review found no remaining issue. Identical public causal test
bytes now discriminate the proper older baseline, uncorrected candidate and
corrected source: the older baseline passes four and fails active loop remapping;
the uncorrected candidate passes three and fails both refusal witnesses; the
corrected candidate passes all five. The independent loop-sensitivity controls
cross actual changed boundaries and differ from bypassed/plain playback.

A first attempted uncorrected run reused the older executable from the shared
Cargo target and is retained as INVALID evidence. The corrected identity run
refreshes tracked input mtimes without changing bytes, requires fresh compiler
artifacts naming the actual worktree, and checks the complete executable test
list. No target directory or compiler wrapper override was used.

The earlier corrected source `3eb7067d7` passed broad all-target Clippy, 558 core tests
with 11 existing ignores, and 168 focused runtime tests with one existing
modulation-cost benchmark ignore. The complete same-policy Doctor scan has
zero producer errors and recorded FAILED with one error and 11,474 warnings.
That remaining error was Kontakt filter state-family pairing. Its follow-through
is now applied and validated below. No audit rule or severity has been waived.

The earlier three-error checkpoint covered two control projections and the
Kontakt filter pairing. These were private constructor/consumer invariants;
no public malformed-input crash was established for them. The macro detector
correctly identified source patterns. Treating every such diagnostic as a
proved crash would be a false interpretation; calling the failed report clean
would also be wrong. Typed ownership must remove the impossible cross-product
without hiding an assertion in an accessor, adding a fallback processor or
changing DSP arithmetic.

The final Kontakt correction binds the actual filter history to borrowed
references to the current prepared kernel for one process call. Matching-family
warm history survives prepared rate/mode/variant changes; different families use
the original initialization. Native equations, 32-frame scheduling, vector
expectations and stored history declarations are unchanged.

The proper baseline and candidate match all 180 exact output-bit digests. The
mixed-generation public runtime fixture compares exact PCM and semantic state
across serial, two-thread and four-thread rendering; its all-thread callback
allocation and free counters remain zero. Independent review found two defects
in the added supplemental test (SVF targets used for Kontakt modulation, and
shared allocation counters in separate concurrent tests). Both were corrected
identically in baseline and candidate before acceptance. A test-oracle module
path compilation failure was also retained and corrected.

At `c41e24dc1`, broad all-target Clippy passes; 565 core tests pass with 11
existing ignores, and 168 focused runtime tests pass with one existing manual
modulation-cost benchmark ignore. The same-policy complete Doctor scan returns
exit zero and passes its error gate. It still reports 11,475 warnings. The one
net new warning identifies a real 1,208-byte by-value history transfer; it is a
reviewed cost, not a false positive or a suppressed diagnostic.

Stored Kontakt history measures 1,208 bytes and the transient typed owner 1,216
bytes. A balanced 11-pair optimized-CI fixture comparison measured median
candidate/parent process-wall and CPU ratios of 0.986 and 0.987. This bounded
workload detected no regression; it includes preparation and test overhead and
does not establish isolated callback deadlines or shipping-profile stack/copy
costs. Those remain final acceptance obligations. No zero-cost claim is made.

Complete reports, source hashes, classifications and executable receipts are
retained in dev-drive PluginArtifacts under `kontra-install-doctor-20261016-r5`.
The latest complete same-policy report is `phase-e-quality01/same-policy-doctor.stdout`,
SHA-256 `eff973536846f1fdb270c6d2eb967cdf83658e676b30ac5b9764d8e8596b559b`.
Exact baseline/candidate receipts are `phase-e-preservation02`; the failed first
compile remains `phase-e-preservation01`. The bounded cost receipt is
`phase-e-cost01`. Earlier causal receipts and the successful GPU receipt
`gpu-owner09-01/receipt.json` remain unchanged. Applied source changes are
committed separately from the installed 0.3.503 binaries. Final combined
validation, VWinds six-channel body restoration, binary replacement and release
remain pending.

PR #76 Linux CI exposed a separate synchronization error in an inherited
diagnostic test. After filling the bounded request queue, the test treated the
independently observed `Parked` phase as a queue-empty fence. A subsequent
nonblocking UI edit could correctly refuse while canceled requests remained.
The correction uses an ordinary FIFO diagnostic request, owner acknowledgement
and command-marker drain before the second edit. Queue capacity, deadlines,
production behavior and the existing refusal assertions are unchanged.

The actual CI failure is retained under `pr76-linux-failure01`; one local
baseline execution did not reproduce the race and is not labeled RED. At
baseline `dd00e59a0`, with the test-only patch SHA-256
`1582ea943d1946d6e83d744f7fc7de32cf5fcb6a11fbe2987edd2cb171c05a68`,
all 15 threaded tests passed with eight test threads, affected all-target Clippy
passed, and the complete same-policy Doctor scan passed with zero errors,
zero producer errors and 11,475 warnings. The final test file SHA-256 is
`2d35b53eecc760c5d532a31524fa33a89e3b2d81bb6d676b6027951140b11ba8`.
Exact feature/source/artifact identities and unchanged-source checks are in
`pr76-diagnostic-fifo01/validation01/receipt.json`. This evidence does not replace
the new PR CI/platform checks or establish a VWinds audio fix.
