# Doctor remediation checkpoint

The same full-policy corrected producer completed with zero producer errors
on both the baseline and this branch. No rule or error severity was waived.
The baseline report failed with 69 error-level findings. The complete report
at `c28041019` failed with 11 and at `4d70b5710` with six. The latest report
at `a263eed7a` fails with three. The typed control-store candidate at
`19eaf9e10` produces one error, but two independent review findings block
acceptance of that candidate.
This is not a clean audit or final release acceptance. The latest report also
retains 11,473 warnings and 85 informational diagnostics.

The reduction of 68 source patterns must not be described as 68 observed
crashes. The audit detects macros and policy violations; it does not prove
public reachability. Findings were separated into malformed-input defects,
private validated invariants, and analyzer interpretation limits before
changing their ownership or refusal boundaries.

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

## Remaining audit error and open review

The control-store candidate passed all-target Clippy, 558 core tests with 11
existing ignores, and 163 focused tests with one existing modulation-cost
benchmark ignore. Its complete audit has one error: Kontakt filter state-family
pairing. This does not establish candidate acceptance. Independent review found
that ordinary non-Real WavetableForm writes lost a historical refusal and that
the loop-remapping fixture never reached an affected boundary. Both require
correction and causal validation; passing existing tests did not expose them.

The previous three-error checkpoint covered the two control projections and
Kontakt filter state pairing. The control changes are still under review.

All remaining sites are currently private constructor/consumer invariants;
no public malformed-input crash was established for them. The macro detector
correctly identifies their source patterns. Treating every such diagnostic as
a demonstrated crash would be a false interpretation, while calling the
failed report clean would also be wrong.

These sites concern wavetable Real-control projection, paired control
domain/value normalization, and Kontakt filter state-family pairing.
Follow-up changes must retain the finite
domain in its actual owner. Moving an assertion into an accessor, adding an
unwrap, fabricating a fallback processor, or duplicating authoritative scalar
state does not resolve the ownership issue.

The complete reports, source hashes, classifications and executable receipts
are retained in dev-drive PluginArtifacts under
`kontra-install-doctor-20261016-r5`. The latest complete same-policy report is
`doctor-typed-d01/same-policy-doctor.stdout`, SHA-256
`3186ce077b0d63c777f07058084c065d417b0b6e5721a0dc24434e3712ebd70b`.
The successful GPU receipt is `gpu-owner09-01/receipt.json`. Candidate code is
committed separately from the installed 0.3.503 binaries.
