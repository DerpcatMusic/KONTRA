# Falcon/UVI parity and worktree recovery

Reviewed 2026-10-12 against v2 `11ca186d`, on `feat/kontakt-falcon-parity`.
Frozen v1 UVI reference: `4bffbb18`; older engine reference: `0cb7a8a0`.
Other worktrees and `/mnt/MAIN_STORAGE/Libraries/UVI` were read-only. Installed
library testing belongs to the [complete scanner receipt](library-results.md); the old
660-program census below is historical evidence rather than today's pass count.

## Fixed: nested effect racks processed twice

`Translation::inserts` collected every descendant `Chains` node of an
`EffectRack`, then recursively translated each chain's inserts. A nested rack
was therefore processed inside its owning chain and again as sibling branches
of the outer rack. Its inner chains could also leak through a bypassed parent.
The fix collects only the rack's direct `Chains` children; recursive insertion
preserves the inner hierarchy.

The [Falcon effects manual](https://manual.uvi.net/falcon/en/effects/) describes
an Effect Rack as an inline effects path whose own chains run in parallel.
Applying that hierarchy to the retained XML is the basis for this routing fix;
no native DSP coefficient law was changed.

The authored PCM regression
`effect_racks_keep_nested_chains_inside_their_owner` uses a constant sample and
an outer gain of 0.5 around two inner gains of 0.25 and 0.75, with an outer
sibling gain of 0.125. Expected total: 0.625 × input. Before the fix, the parent's
baseline run measured input 0.48828125 and output 0.79345703125, or 1.625 × input.
After the fix, the static and scripted cases pass at 0.625; bypassing the parent
leaves only the sibling at 0.125. The same test exercises nested parallel
processing in the shared core, rather than merely counting translated nodes.

Validation: `/home/derpcat/.cache/kontakto-heavy cargo test -p sampler-uvi
--profile ci --test fixture effect_racks_keep_nested_chains_inside_their_owner
-- --exact`: **1 passed, 0 failed**, 0.01 seconds after compilation. The parent
baseline had also passed the other 22 fixture tests. The final complete suite
and installed-library results are recorded in the [overall audit](README.md).

## What has already been consolidated

The following comparisons examine final source content, not just Git ancestry.
The branches diverge from main because historical integration was squashed;
their old commit IDs alone do not prove a feature is missing.

| Retained work | Current v2 evidence | Recovery decision |
| --- | --- | --- |
| `gpt/parity-luau`, `7c29f7cc` | Synthetic Part readback and explicit unknown module references are already in `parity.rs`, `parity/ledger.rs` and their tests. Current source also adds `report_sources`. | Preserve branch; no duplicate cherry-pick. |
| `wp123-uvi-parity`, `4f7cfeee` | `inserts.rs`, `modulation.rs`, `reverb.rs` match the branch's final DSP content before this audit fix. `engine_parameters.rs` differs only by exposing `lanes` for later parity reporting. | First-order shelves, program/layer modulation, LFO ramp/S&H mapping, bipolar script sources, impulse shaping and live convolution controls already landed. |
| `uvi-ui-lua-fixes`, `e0710327` | Supported DigitalEq Bandwidth/GainScale writes, insert bypass, constant promotion and one-to-many control bindings are present in current source and rendered fixtures. | Already superseded by the consolidated code; no branch merge needed. |
| `parity/medium-uvi-20261011`, `14f89598` | Absent parameter connections return nil; the normal `connections_do_not_fabricate_entries` fixture verifies that behavior. | Already landed; retain as history. |
| `pi/uvi-loaddata-async-20261010`, `c27a6a86` | `script/async_data.rs`, load-data tests and Lua owner receipt handling are in current v2. | Consolidated; preserve history and native-law limits. |

## Dirty UVI worktrees

These were inspected without editing, committing or deleting their files.
No reliable conversation record explains every abandoned edit, so the decisions
below describe source evidence instead of attributing intent.

| Worktree | Uncommitted UVI work | Evidence and action |
| --- | --- | --- |
| `decipher-readers-v1` | `crypto.rs`, `ufs.rs`, UI/worker fixture headers | Current v2 already reads `encoded_names` and `root_entry_offset`, handles clear versus encoded directory names, links the real root, and has the corresponding synthetic directory regressions. Do not import old files wholesale; preserve any independent v1 crypto experiments for separate review. |
| `fix-uvi-discovery` | Installed-reader discovery, explicit reader overrides, reader validation | Current native bank access no longer depends on locating an external reader. These old APIs target the superseded loader contract; merging them would restore obsolete settings paths. Preserve the worktree, without merging this patch. |
| `gpt-uvi-ui-baseline` | `ui_properties` instrumentation and scratch examples | Observational property counters only; current UI projection and image geometry tests are broader. No production fix to recover. |
| `audit-uvi-ui-probe` | Untracked `tests/audit_resources.rs` | Count-only resource-path diagnostic, tied to old reader prerequisites. Keep as research; current Resources and UI health tools already expose production resolution. This scratch test was not copied or executed. |
| `v2-uvi-fix` | `script_playable_keys` helper and corpus-health changes | The helper is absent from current v2. It sequentially auditions keys 24–108 on one throwaway host; it does not prove each note reached audio or reset state for each key. This is a useful scan idea, requiring controller/articulation context, isolated audition state and an explicit distinction between sample range, script command emission and audible PCM. No blind merge. |

## V1 strengths that v2 still lacks

V1 has actual executable UVI DSP behind explicit admission gates, not just
catalog metadata. `4bffbb18:src/uvi/playback.rs` admits supported subsets from
`effects`, `filter`, `time_effects`, `waveshaper`, `maximizer`, `sparkverb`,
`phasor`, `biquad`, `compexp` and `generator`. V2's insert translator has a much
smaller working set. The presence of a file or measured leaf does not establish
full native program parity.

* **Channels:** v1 `dsp.rs` defines a 12-channel Frame and channel-count-aware
  processing. V2 audio decode, streaming and shared runtime keep stereo. VWinds
  matrices that route channels 3–6 to output 1 lose those sources before the
  present 2×2 matrix runs. This requires an end-to-end channel contract; adjusting
  stereo coefficients cannot restore discarded audio.
* **Admitted filters/effects:** v1 contains supported XpanderFilter, SparkVerb,
  time-effect, phasor and other processor implementations with retained native
  observations and explicit limits. V2 reports those XML modules as dropped.
  Port one admitted subset at a time into shared IR/core, keeping rate, block,
  parameter and smoothing gates plus authored reference vectors.
* **Isolated kernels:** Comb, MS20, Drive and Diode work includes measured leaf
  behavior, but v1 playback preflight explicitly rejects those leaves when
  connected-control or lifecycle admission is unverified. Recovering a kernel
  alone must not promote it to whole-program parity.
* **Native control graph:** v1's `modulation.rs` retains native scoped control
  relationships and law evidence. V2 folds admitted curves/routes into the
  shared engine and reports unmodeled live frequency, nested source and
  retrigger cases. Preserve shared-instance and timing semantics when porting.

V1 native comparison claims above are retained source/evidence claims, not
new comparisons performed by this audit. `0cb7a8a0` predates the large UVI
addition at `4bffbb18`; treating it as the only v1 reference would miss the
stronger UVI work entirely.

## V2 strengths and remaining limits

V2 centralizes sample playback, streaming, MIDI, control bindings, scripted
thread ownership and UI IR. It already supports authored Lua/widget execution,
supported live GainMatrix/DigitalEq/shelf/convolution controls, scripted insert
bypass, scoped program/layer connections and generated-note/oscillator mapping.
The parity lexer/ledger attaches supported, approximate and stub status to
source spans, and the script diagnostics keep source/resource locations.

Important remaining limits include native Multi/Part execution, multichannel
audio, non-sample Falcon oscillators, most native effect modules, voice-scope
convolution, admitted live modulation laws and full per-bank UI fidelity.
DigitalEq types 0–4 retain a numbering warning; positive SampledReverb damping
remains reported. Synthetic `Program.parent` fields provide readback without
native Part DSP writes. Program Polyphony does reach shared voice stealing,
contrary to the old coverage document; other program policy fields remain
partial or unbound.

The old `FALCON_MODULE_COVERAGE.md` incorrectly said every insert was dropped,
and README/FEATURES still said Falcon had metadata inspection only. This audit
updates those claims from current executable paths, while retaining experimental
playback and native parity limits. The 660-program module occurrence counts are
now labeled historical; they must not stand in for the current installed corpus.

## Test coverage and scan interpretation

`tests/uvi_audit.rs` contains **31 ignored authored conformance probes**, all
using generated XML, synthetic PCM and host time advancement. They require no
installed libraries or reader paths and may be run serially with
`--test uvi_audit -- --ignored --test-threads=1`. Their old ignored labels are
not current failure results. Some assert historical audit expectations; a new
failure must be checked against current vendor/v1 semantics before changing the
runtime. Real-bank and catalog surveys are separate ignored targets with explicit
data prerequisites and are not part of that command.

Two historical audio leads need fresh reproduction: VWinds Flutes Alto had a
silent matched-note scan at key 60/velocity 64 despite 22/22 bound controls in
the retained `840341e…` scanner observation, while an older `e9d918…` run was
audible; a Kontakt Conflux multi had a silent first audition and audible second
audition. These are scan/playback leads, not native PCM comparisons. The parent
audit's new full installed-library scan determines their present status.

Unreadable Augmented Orchestra banks with zero-filled headers or child tables
must be counted as container failures. They cannot demonstrate a script or DSP
regression because translation never received a valid program. Conversely,
successful bank indexing, a ScriptHost producing commands, nonzero PCM and
native Falcon sound equality are four different outcomes; retain them separately
in future receipts.

Recommended next integration order: preserve recoverable worktrees, land the
verified nested rack fix, reproduce remaining scan failures with deterministic
event/controller context, restore one v1-admitted DSP subset with its evidence
gates, then implement the multichannel contract across decode/stream/core/UI.
