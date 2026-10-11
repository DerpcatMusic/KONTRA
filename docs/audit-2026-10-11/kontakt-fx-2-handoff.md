# Branch + worktree path + commit SHAs (oldest first)

Package `kontakt-fx-2`, branch `gpt/kontakt-fx-2`, worktree
`/home/derpcat/.t3/worktrees/KONTAKTO/gpt-kontakt-fx-2`, base `070e8e91`.

Research-only commits: `6f54a4ce`, `6996b12f`, `29d0900f`, `29da6da0`,
`d21bb064`, `4a1d569f`, `e11a5d81`. This receipt is the following commit.
No push, PR, merge or integration-worktree mutation.

# Per item: before → after, root cause, evidence (test names, measurements, file:line)

No item has an implementation change. Vendor documentation was read, the
historical analysis cache was traced, and mandatory live MCP extraction
was attempted. REA accepted the original executable identity but failed
before returning any function; radare2 MCP could not open its hidden path.
The exact tool observations, SHA, run and profile IDs are in
[`kontakt-fx-2-evidence.md:10`](../re/kontakt-fx-2-evidence.md#kontakt-fx-round-2-evidence-boundary).
Cached addresses below are provisional leads, not successful MCP evidence.

| Item | Before → after; root cause | Research evidence |
| --- | --- | --- |
| Morphology | Unresolved saved 35/41 and 13/39 → unchanged. LP-only output selection cannot express the other ladder rows. | `docs/re/kontakt-morphology.md:9`: saved-ID conversion `0x140cf4c80`, audio `0x140b00b80`, weights `0x144fcebc0`. PE table checks yield exactly .25/.375/.5 for 35/39/41 with (1,.5,.25,.125,.0625). Type 13 has a separate cached filter kernel `0x140ae9f90`. |
| Formant I | Serial vowel proxy → unchanged. Cached native topology is three parallel bandpasses with different tables. | `docs/re/kontakt-formant.md:9`: setter `0x140b05ac0`, audio `0x140b00750`, tables `0x144fcea10/90`, `0x144fceb10`. Provisional first impulse sum .01663771558 at 48 kHz, Talk 0, unnamed core controls .5/.5. Saved Sharp/Size assignment remains unresolved. |
| Inverter | Guessed flag order, diagnosed → unchanged. Missing BPar→audio-wrapper assignment prevents settling serialization. | `docs/re/kontakt-inverter.md:9`: reader `0x140d01db0`, audio `0x140908e60`. First/second bytes land at BPar+8/+9; audio invert/swap are wrapper+0x1c0/+0x1c1. No connection proved. |
| Phaser | Legacy slot unimplemented → unchanged. Native LFO/control mapping is incomplete. | `docs/re/kontakt-phaser.md:10`: reader `0x140d01fa0`, audio `0x140aa3830`; cached four-stage topology conflicts with using the v1 six-stage proxy as proof. |
| Reverb / convolution / alternate compressor | Approximations or unsupported controls → unchanged. Setter values and generic convolution do not identify complete native algorithms. | `docs/re/kontakt-reverb.md:9`: unrecovered dispatch `0x141ae2c60`; `docs/re/kontakt-convolution.md:8`: filter API `0x1419b3a70`; `docs/re/kontakt-compressor.md:10`: alternate detector `0x140ab2c80` differs from generic detector. |
| Instrument bus pan | Dolce pan unsupported → unchanged. No instrument-bus audio pan function identified. | `docs/re/kontakt-bus-pan.md:10`: BInsertBus record vtable leads only. No gain curve or pan vector claimed. |
| Live Shape/Drive/Damping | Initialization works, playback controls unconnected → unchanged. Processor roles/inputs and native live update behavior still needed. | `docs/re/kontakt-drive-parameters.md:10`: BEffectShaper `0x140aa4d20`; existing lower/control binding path located. No generic-routing or registry-internals edits. |

Documentation citations: NI [Filter Reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/filter-reference),
[Effect Reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/effect-reference),
[KSP Engine Parameters](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters),
[Bus Insert Effects](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view).
Each module note names its exact relevant sections. Historical binary source:
`t3code-80fe786b/artifacts/engine-analysis-2026-10-07/kontakt-engine/`.

Validation: one workspace `kontakto-heavy cargo test --no-run` passed,
exit 0, reported 54.69 seconds; existing warnings remain. No code changed
afterward. Morphology table arithmetic checks passed. No focused production
test was added or run; failing-first is not applicable because there is no
fix. Formant arithmetic uses double precision and is not a native f32
comparison. No full suite, corpus survey, reference recording or audio
parity measurement was performed. No diagnostic was removed or downgraded.

# Files touched outside your scope (and why)

None. Only requested `docs/re/` research notes and this handoff receipt
changed. No IR, validation, UI, DSP, script, modulation or library files changed.

# Still open: item + concrete reason

All seven implementation items remain open. Shared blocker: required
REA/Ghidra extraction failed twice before serving a function, and radare2
MCP rejected the binary path. The implementation protocol explicitly
forbids substituting guessed laws or cached interpretation for that gate.
In addition, each note identifies its missing mapping/topology above.

# Anything the coordinator must act on (conflicts, follow-ups)

Provide a working REA analysis snapshot matching the recorded SHA/provider/
profile, or supported MCP access to the executable. Retry Morphology first,
then Formant; their table/topology leads are the most concrete. Do not treat
these commits as DSP parity fixes or diagnostic closures. There are no
source conflicts introduced; coordinate new live parameter roles with
`modulation` before implementation. User clarification requesting supported
MCP access remained unanswered at handoff.

Own compile job completed; REA session closed. Own target and scratch are
removed at handoff. No dev server, Wine host or recording process was started.
Graft had no index in this newly created worktree: one query, zero reported
token savings; normal lookup was used after that result.

Skipped: DSP implementation and native-host confirmation because mandatory
binary access failed. Risk: all existing unsupported behavior remains.
