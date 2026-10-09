# Native integration batch 0.3.344

Tested production source: `1359d55cbcdc6cc76073b8ed4cdf927577310996`.
The final acceptance commit changes this receipt and ledger validation only.

The batch includes the native UVI bank/PasswordV2 provider, bounded callback
work and coroutine wakeups, complete readback publication, accurate grouped
bank failures, KSP MIDI-object completions, pre-insert Tone, Mapping/Sound,
keyboard/popup/Info/EQ fixes, partial GPU vector repaint, permanent Nightly
publication, grouped audit tooling and the shared biquad kernel. The kernel
is an internal refactor with no coefficient/control law change. Mapping and
audit additions do not increment the defect count. Eighteen reviewed logical
fixes advance the ledger from 326 to 344.

## Validation

- Final locked ci/shots workspace no-run passed.
- Workspace: 1,811 passed, zero failed, 215 ignored across 174 executable and
  doctest groups; root: 433 passed, zero failed, 29 ignored. Nested journal
  subprocess results are excluded from the total.
- One integration RED came from Mapping's obsolete assertion that Falcon
  source maps must be empty. The native parser supplies player-to-zone IDs;
  the fixture now checks IDs 1/2/3 and retains waveform/layout/audition checks.
  Focused GREEN: 1/1; full workspace then passed.
- Native-only optimized quick: 90 loads, 80 audible, 80 healthy stages;
  all 20 UVI bank programs opened. Zero new stage failures, nonfinite audio
  or audio-thread allocation calls against the existing 326 receipt.
  Ten existing note/selection silences are unchanged. Three stored long
  releases are reported separately from faults.
- Offline scanner/grouped/census/contention/presentation/load-host and CI
  checks passed. Publisher checks retain oversized Unicode full-note attachment
  coverage alongside permanent-history/out-of-order publication tests.
- Provider source `8f4c3c7c7f268003a40bd34b6eff1b478cbb1650`, PR46: all eight
  hosted checks passed. Windows executed native UVI bank and Kontakt access
  fixtures successfully. Linux additionally opened/played the installed
  Augmented Orchestra bank with an isolated HOME and no available reader/Wine.

## Evidence and limits

Receipts: `~/.cache/kontakto-w0/native-batch-344/`:
`workspace-summary.json`, `workspace.log`, `final-no-run.log`,
`quick-verdict.json`, `quick-native.jsonl`, `quick-historical-diff.md`.
Provider receipts: `~/.cache/kontakto-w0/native-provider/`.
Source cherry-pick identities: `~/.cache/kontakto-w0/native-batch-preparation.json`.

The historical 326 quick predates the native-only directive and used external
UVI namespaces. It was read only; no reader-based baseline was rerun. This is
an admission/stage check, not same-protocol PCM or timing certification.
Existing matched seeded PCM receipts permit only the unchanged W8 `ed07a549`
chain implementation; audit `bb36f954` ancestry remains excluded.
W9 streaming and the W15 registry stack awaiting complete READY remain excluded.
Quiet CPU/load/RSS, full presented-editor parity and real installed-library
execution on Windows remain unmeasured. Native Windows fixture execution passed.
Installed 0.3.306 remains untouched. Local artifact freezes are not installs;
local glibc may exceed the Ubuntu22.04 shipping baseline.
