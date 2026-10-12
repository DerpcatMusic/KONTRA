# Installed-library results, 2026-10-12

The offline catalog covers **1,495 unique installed items across 15 libraries**:
781 NKI instruments, 54 NKM multis containing 113 physical programs, and 660 UVI
programs from 26 banks. Every manifest ID has exactly one report. All 60 shard
receipts match the manifests, output digests and frozen inspector binary; there
are no missing, duplicate, unexpected or malformed reports.

Every source report is **partial**. All source inspections completed, and all
660 UVI programs reached production XML-to-IR conversion. Those facts establish
inspection coverage, with reported semantic omissions still present. Native
Kontakt/Falcon behavior remains **unknown**: no new native-host capture was run.

## Separate evidence stages

| Evidence | Result | Scope |
| --- | --- | --- |
| Source catalog | 1,495/1,495 | All physical Kontakt programs/script slots and all enumerated UVI program sources |
| KSP frontend reports | 4,470 ok | Includes physical empty/bypassed slots; KONTRA frontend, no native compiler |
| Luau syntax checks | 780 ok, 0 rejected | Program/module occurrences; compiler only, no native Lua 5.1 semantic proof |
| Static UVI IR | 660/660 | Actual production translation; unsupported fields/laws remain reported |
| Initialized selections | 17/17 initializer and IR stages ok | Every library, plus Conflux NKM control and musical programs; zero explicit initializer faults |
| Initialized UI models | 9 ok, 8 partial | 857 unsupported UI entries across selections; entries are not unique missing widgets |
| Headless load/audio | 16/16 loaded and audible | One selection per library plus a Conflux multi; one bounded note plan, no native audio comparison |
| Headless renderer | 13 ordinary paints, 2 missing-image results, 1 blank result | Current KONTRA renderer, independent of UI-model success and native pixels |
| Native parity | Unknown | Requires versioned native states, callbacks, audio and UI captures |

The initialized UVI runs also reported 110 `LuaFinding` entries. Their IR now
uses the shared production translation after bounded `onLoad` overrides; the
previous inspector paired initialized UI with pre-initialization IR. An authored
regression verifies an override in the resulting IR. Compact installed reports
do not expose parameter values, so they cannot prove each individual override.
An initializer
can complete while an operation is unsupported or approximated. The five failed
Kontakt placeholder lowerings used deliberately fictitious 48kHz/2^32-frame
assets; they are not real sample-load failures. Eight such placeholder lowerings
passed, and UVI did not attempt this placeholder path.

## Per-library coverage

The table below is generated from the reconciled source aggregate and separate
headless receipts. “UI model” counts selected programs, not all library presets.
All native comparisons remain unknown.

| Library | Source items | Static UVI IR | UI model ok / selections | Headless audible / selections | Renderer issues |
| --- | ---: | ---: | ---: | ---: | --- |
| ANALOG STRINGS | 1/1 | Not run | 1/1 | 1/1 | No reported paint/lookup issue |
| Afflatus Chapter II Brass | 348/348 | Not run | 1/1 | 1/1 | No reported paint/lookup issue |
| Areia 1.2.0 [Audio Imperia] | 155/155 | Not run | 1/1 | 1/1 | No reported paint/lookup issue |
| Audio Imperia CHORUS | 42/42 | Not run | 1/1 | 1/1 | No reported paint/lookup issue |
| Audio Imperia Dolce | 77/77 | Not run | 1/1 | 1/1 | No reported paint/lookup issue |
| Conflux 1.1.0 [Native Instruments] | 51/51 | Not run | 0/3 | 2/2 | blank |
| Morphology Evolved [Zero-G] rutracker.org | 1/1 | Not run | 0/1 | 1/1 | No reported paint/lookup issue |
| Pacific Ensemble Strings | 50/50 | Not run | 1/1 | 1/1 | missing-images |
| Performance Samples Vista | 7/7 | Not run | 1/1 | 1/1 | missing-images |
| Solo | 100/100 | Not run | 1/1 | 1/1 | No reported paint/lookup issue |
| Una Corda Library | 3/3 | Not run | 1/1 | 1/1 | No reported paint/lookup issue |
| UVI - Augmented Orchestra v1.1.2-R2R | 620/620 | 620 | 0/1 | 1/1 | No reported paint/lookup issue |
| VWinds - Clarinets | 16/16 | 16 | 0/1 | 1/1 | No reported paint/lookup issue |
| VWinds - Double Reeds | 15/15 | 15 | 0/1 | 1/1 | No reported paint/lookup issue |
| VWinds - Flutes | 9/9 | 9 | 0/1 | 1/1 | No reported paint/lookup issue |

Conflux's `A Forgotten Romance.nkm` program 0 is control-only with zero converted
zones. The extra initialized program 1 has 1,985 zones. Inspecting program 0 alone
would not establish a playable multi. The headless scanner separately loaded the
multi's two programs and measured an audible note, with a blank renderer result.
That does not certify every NKM program or gesture.

Pacific requested five image lookups and resolved four; Vista requested four and
resolved three. The missing-image classifications are retained. Their initialized
models reported no UI projection omissions, illustrating why model projection,
resource resolution, rendering and native pixels require separate gates.

The fresh Alto Flute selection is audible. An additional four-second, note-60,
velocity-64 probe produced 384,000 finite interleaved samples, nonzero output, and
zero reported fault/stream/underrun counters. It retained only a numeric hash
(`3152004e504fca0af13a198753e2aba2c1a724f109cb3eefdb3d65986c28d63f`).
This supersedes a historical short silent result for this selection without
establishing native timing or explaining every historical protocol difference.

## Gaps and remaining installed failures

The static frontend reports contain 83,960,006 Approximate, 10,694,039 Implemented,
4,365 Stub and 1,490 Unsupported construct **occurrences** across 5,250 physical
script/module reports. Presets frequently repeat the same code; these are not
unique API counts. “Implemented” means a concrete binding is available, with
native behavior requiring separate evidence. No item qualifies as source
semantics without reported gaps. Translation diagnostics similarly repeat
1,843,943 NotModeled and 167,544 UnknownLaw entries.

Earlier explicit installed-library tests retain these unresolved results:

- Afflatus `2 Horns Staccato.nki` microphone-node audition stayed silent across
  four attempted keys, with 304 suppressed starts and no reported runtime fault.
  The fresh audible `2 Horns KS.nki` selection is a different patch and does not
  resolve this failure.
- Contrabass Clarinet produced finite, nonzero but quiet peaks around 0.00164–
  0.00173. Three root tests failed their unchanged >0.01 prerequisite. Their
  downstream pitch-bend, allocation and purge assertions remain unverified.
  The fresh audible A Clarinet selection is a different program.
- AO `City Walker` remained silent in its scripted installed-bank test. Other
  AO selections now produce audio; Bartok's fresh success does not resolve City
  Walker.

The narrowed VWinds impulse/asset-index panic was reproduced and repaired, with
the installed witness reaching peak 0.026274467. Big Screen's unconsumed script
outbox was repaired: its exact audition changed from peak zero to 0.05534125.
Analog Strings pedal/tail regressions passed after the same host-effect repair.
See [integration review](integration-review.md) and
[runtime validation](runtime-validation.md) for exact local contracts and receipts.

Native asynchronous `loadSample`, sample-accurate callback scheduling, complete
physical UVI source identities, omitted NCKP/UI fields, and audio/UI publication
acknowledgment remain substantive work. The [native loader design](loader-design.md)
separates existing mechanisms from proposed changes; the official-contract audits
for [Kontakt](kontakt-native-contract.md) and [Falcon](falcon-native-contract.md)
define the next native probes. Both historical KONTRA implementations can encode
the same mistake.

## Integrated changes and validation

The branch integrates the offline workbench, positioned compiler/init/UI
diagnostics, one-time Kontakt UI initialization reuse, documented UVI restore
order, source-safe cache refusal, reviewed W13 UI recovery, nested/silent rack
ownership repairs, headless effect servicing, linear solo-mask computation,
and the UVI impulse identity repair. Source inspection now avoids duplicate
frontends/decodes and repeated linear node searches. These are scoped local
improvements, with their native boundaries retained.

| Check | Receipt |
| --- | --- |
| All 14 first-party packages | Final post-initialization run: 2,227 passed, 0 failed, 153 ignored |
| Clippy, all 14 packages/all targets | Exit 0; pre-existing warnings remain |
| UVI scan coverage | 4/4, including unvisited modulation targets previously mislabeled as admitted |
| UVI authored DSP fixtures | 24/24 |
| Inspector authored fixtures | 19/19, including malformed KSP, positioned initializer failure, NCKP omissions, malformed Lua and post-`onLoad` IR overlays |
| Aggregation | 15 synthetic integrity/evidence tests |
| Launcher | 5 tests; actual Conflux browser checks verified source jumps, lowering, original NCKP, partial UI and no horizontal overflow |
| Recovery and historical aggregation | 2 worktree-tool tests and 7 fail-closed historical-evidence tests |

An earlier integrated run found a stale generated-ledger fixture; refreshing
seven source-position rows preserved every ledger status/binding. Its affected
target passed 8/8, and the final full run above also passes. This is
authored/default-environment coverage. It must not be presented as a fully passing
explicit installed-library suite: the failures above remain unresolved.

Local logs reside in `/tmp/kontra-parity-audit-20261012/`, including
`final-postinit-authored-tests.log`, `final-ledger-parity-green.log`,
`final-postinit-clippy.log`, `final-runtime-scanner-build.log`,
`runtime-host-diagnostic.log`, and `alto-flute-four-second-hash.log`.
Library roots were read-only. Samples and protected source were not exported;
optional original source in the running workbench is held in memory.

## Durable source-free evidence

- [Source aggregate](library-source-summary.json): every library denominator,
  status/IR/compiler counts and all 60 report-file digests.
- [Initialized selections](library-initialization-summary.json): the 17 selected
  program identities, fields, omission counts and verified receipt digests.
- [Headless aggregate](library-runtime-summary.json): 16 load/UI/audio receipts,
  explicit remaining preset coverage and matched-note-plan/native-family limits.

The source-only catalog uses source commit
`c4525f338a4bdcd04214f39f75be03856bdc5dab` and CI binary SHA-256
`fa7bd6013395c8b490c2f1d98457676496dac456b1956fabf4df7fc52622578d`.
The 17 initialized selections use source commit
`3140ccc90016a9bd2eaa04d927c3c01290a31f6c` and a separately frozen CI binary
SHA-256 `b7606c4e2900c419d5798dbdd28e5c99ca79f59356b207560769a763dcdd7a9c`.
That change corrects initialized UVI IR; the static path is unchanged.

Manifest SHA-256 is
`02be1aa7cc78c41557e700628b106b95fa7ade3e2cccbc29af61309175158092`.
The independent headless scanner has SHA-256
`f7faedca5d27645f08c0c9410ef86eb03a7df98665f3902a05014973206432a6`;
its build predates the shared UVI inspection-helper refactor and retains its own
provenance. Raw source-free reports remain in the private RAM audit workspace;
the repository contains aggregates rather than protected code, resources or PCM.
Intermediate partial scans and superseded initialized reports were preserved
and excluded. Each final evidence stage identifies its own frozen artifact.

## Reproduce

Build `corpus-health` through `/home/derpcat/.cache/kontakto-heavy`, regenerate the
installed manifest using its `list` command, then collect `inspect-list` JSONL
under read-only mounts and scan mode. Reconcile it with
`tools/inspector_audit_summary.py`; provide initialized selections separately
through repeated `--initialized-report` arguments. The
[inspector guide](../library-inspector.md) documents source opt-in, sandboxed
initialization and workbench use. Raw library/code/audio artifacts stay local.
