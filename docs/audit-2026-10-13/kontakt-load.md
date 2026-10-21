# Kontakt load package, 2026-10-13

Branch `gpt/kontakt-load`, worktree
`/home/derpcat/.t3/worktrees/KONTAKTO/gpt-kontakt-load`, base `ce0a2e61`.
Four production corrections are implemented. Full Kontakt parity is **not
established**. The user stopped builds and corpus work; remaining corpus counts
and all-library cold/warm measurements belong to the coordinator.

## Evidence scope

The existing [inspector catalog](library-source-summary.md) reconciles all
**835 files / 11 libraries: 781 NKI, 54 NKM**. Its static Kontakt inspection
reads every program; it does not perform every file's product translation,
sample admission, playback or native comparison.

Before the stop, this package completed fresh production translation for
**40 files: ANALOG STRINGS plus 39 Afflatus presets**, totaling 4,087 groups,
260,352 zones and 169,696 assets. All 40 translated, with zero unmapped groups
or zones. Twenty have additional DSP-slot coverage; the first twenty used
an earlier audit build without `scan`. A separate ANALOG STRINGS cold stream
probe also has DSP coverage. Do not add its counts to the same instrument's
source counts.

The existing initialized reports add **10 non-Conflux representative
selections**, one for each other library. All ten retain their source groups
and zones. These are selected-patch observations, not per-library censuses.
Conflux-specific behavior is excluded from findings and fixes; its existing
51-file static coverage is retained in the denominator.

[Machine-readable evidence](kontakt-load-evidence.json) retains exact per-file
counts, both audit-binary hashes, report digests, representative findings and
the completed stream probe. No library source, raw chunk bytes, samples or
access data is committed. The original read-only RE cache directory named in
the handoff is absent; committed RE research and the SHA-identical native
binary copy were available.

## Ranked load-stage work

Scores are **reach × impact**, with impact 5 for a lost selection/polyphony
rule, 4 for dropped DSP/source behavior, 3 for unresolved native DSP laws,
2 for load allocations and 1 for articulation-migration metadata. This is a
**provisional lower-bound ranking**, not an invented census of translation
for all 835 files. `40` means the new translation cohort; `10 reps` means the
existing non-Conflux initialized selections. Byte retention alone has unknown
audio impact and is not assigned a speculative score.

| Rank | Item | Measured reach / impact / score | Before → after | Disposition |
|---|---|---|---|---|
| 1 | Duplicate whole child-section allocation | Shared structured reader used by all 835 catalog files / 2 / 1,670 | Focused requested allocation 4,194,448 → 2,097,284 B for 2,097,164 B of child data | Fixed; corpus RSS delta pending |
| 2 | Assigned native voice groups without explicit overrides | 38/40 files, 304 unbound-assignment diagnostics / 5 / 190 | Focused missing shared limit → bound native default; valid/invalid corpus after split pending | Fixed from native evidence |
| 3 | Legacy one-band EQ lifecycle | 39/40 files, 2,196 diagnostics / 3 / 117 | Corpus after count pending | Reserved native-EQ/effects worker |
| 4 | Native multiband EQ | 22/40 files, 933 diagnostics / 3 / 66; also 6/10 reps, 112 diagnostics | Corpus after count pending | Reserved effects/native-EQ worker |
| 5 | Reverb algorithm | Four reps: Areia, CHORUS, Dolce, Solo / 3 / at least 12 | Unchanged implementation; after count pending | Algorithm law/dispatch, reserved effects file |
| 6 | Module-parameter modulation routes | 1/40 file, 6,065 diagnostics; 3/10 reps, 6,081 diagnostics / 4 / at least 12 | Unchanged implementation; after count pending | Native destination/control laws and reserved FX translator required |
| 7 | Unsupported FX | 1/40 file, 18 diagnostics; 3/10 reps, 62 diagnostics / 4 / at least 12 | Unchanged implementation; after count pending | Reserved FX translator and native kernels required |
| 8 | LFO waveform admission | 1/40 file, 167 diagnostics; 2/10 reps, 173 diagnostics / 4 / at least 8 | Unchanged implementation; after count pending | Exact waveform/sync laws still needed |
| 9 | Convolution controls | Two reps: early/late filtering 4 diagnostics, IR size 2 / 3 / at least 6 | Unchanged implementation; after count pending | Native IR preparation laws, reserved translator |
| 10 | Keyswitch migration recognition | 6/40 files / 1 / 6; two representative reports | Unchanged implementation | Migration limitation; not proof that retained KSP callbacks fail |
| 11 | Wavetable source fallback | Morphology representative: 30 groups / 4 / at least 4 | Unchanged implementation; after count pending | Non-admitted form/modulation laws; do not guess or suppress |
| 12 | Filter selector and compressor mode | Una Corda representative: 15 filter selectors + 1 compressor mode / 3 / at least 3 | Unchanged implementation; after count pending | Native laws and reserved translator |
| 13 | Archive-member case identity | No corpus loss established; three authored spellings / 4 / corpus score unknown | Two alias identity mismatches → zero | Fixed by porting v1 directory identity |
| 14 | Moved NKX/NKR prefix resolution | No corpus loss established; authored relocation fixture / 5 / corpus score unknown | One missing fixture → resolved; ambiguous mic archives still rejected | Fixed with library-bounded suffix search |
| — | Unparsed/unclassified state | Retained tails in 40/40 files; exhaustive prior declaration inventory in KONTAKT_FORMAT_GAPS | Retained; unknown semantics remain | Retention totals include interpreted fields too; not an audio-failure count |

The scores for ranks 5–12 use the largest proven reach in either cohort,
without adding overlapping observations. These items cannot be totally ordered
corpus-wide from these cohorts. Their
representative evidence establishes additional affected libraries without
extrapolating the selected patch to every file in that library.

ANALOG STRINGS' cold probe records **seven enabled dropped FX slots** (five
legacy Delay, one legacy Chorus, one Limiter), **5,928 enabled dropped
modulator slots**, and **7,645 enabled dropped target records**. These counts
include partially connected modulators and source-not-executed targets;
parent and target counts overlap. `enabled` reflects bypass/mute state, not
proof of nonzero intensity or of audibility in the selected articulation.
Approximated/native-law-unverified, disabled and malformed outcomes remain
separate in the JSON.

## Mappings, loops, routing and resolution

No missing sample, invalid loop, skipped loop mode, empty trim, source-group
loss or source-zone loss was reported in the 40-file cohort or ten non-Conflux
initialized selections. This does not establish decoded-sample or native
sound parity. The prior 834-full-object census observes zero non-neutral
program key bounds/high velocity, group MIDI channels, loop count and loop
tune; current code consumes gain/tune/ranges and loop slots. Its historical
`decoded-unused` labels cannot be carried forward as current defects.
Program low velocity is 1, not an audible velocity-zero note. In the 40-file
cohort program ranges are 0–127 keys / 1–127 velocities.

Group-private bus routing is already consumed; Areia intends separate mic
buses, and Una Corda separates tone/noise buses. Archive relocation must
preserve their source archive/member identities. The relocation regression
uses duplicate Close/Far archives and refuses a basename-only choice.
Source/private extensions, unfamiliar source modes and newer FX parameters
remain explicit gaps; scalar retention does not certify their DSP use.

## Implemented fixes and focused validation

- `e2dd5c63`: `StructuredObject::read` reads each bounded child directly into
  its final owned payload. Private/public regions, unknown IDs/raw bodies and
  the next enclosing record are preserved. The allocation regression failed
  first, then passed with a **2,097,164 B / 50% reduction** in requested bytes.
  Every truncated prefix and a child crossing the section boundary is rejected.
  See `vendor/ni-file/src/kontakt/structured_object.rs:45` and
  `tests/container_allocations.rs:10`.
- `d29003e1`: directory spelling, rather than saved case aliases, determines
  NKX/NKR asset identity. This ports `0cb7a8a0:src/import.rs`
  `Resolver::member_path`. Moved prefixes resolve the archive by the existing
  longest unique suffix rule before resolving its member. Both new tests failed
  first and pass; library-boundary/path-cache checks also pass. The focused
  authored NKR impulse regression passes with canonical identity. See
  `src/samples.rs:195`, `src/samples.rs:759` and
  `tests/sample_resolution_allocations.rs:31,53`.
- `be0a7c5b`: absent voice-group overrides acquire the predefined native
  1-voice/oldest/prefer-released/10-ms rule when assigned. Assignments share
  the same physical limit; unused defaults allocate nothing. Explicit settings,
  invalid/unassigned references and authored mask holes are preserved. The
  focused test failed first and passes. See `src/library.rs:1228` and
  `src/voice_group_default_tests.rs:38`. [Native evidence](../re/KONTAKT_VOICE_GROUP_DEFAULTS.md)
  cites the exact NI section, Ghidra research and independently verified
  radare2 instructions. No fade or stealing DSP algorithm changed.

The allowed `cargo check -j 4 -p corpus-health` passed, also checking its
Kontakt dependency with `scan`/`native-ui`. There was no full workspace
validation after the user stopped that unit; no pending full gate is described
as passed. No new build, corpus, survey, render or performance run was started
after the NEVER BUILD rule.

## Load time and RSS

Only the ANALOG STRINGS cold representative completed before cancellation:

| Metric | Measured value |
|---|---:|
| Production read/translation | 727.6 ms |
| Read + full-key lazy stream readiness | 4,755.4 ms |
| RSS after read | 270.66 MiB |
| RSS at readiness | 364.11 MiB |
| Peak RSS | 433.54 MiB |
| Resident sample heads / page pool | 0 / 24 MiB |
| Fully decoded sample footprint, if resident | 50,156,846,256 B |

Conditions: optimized CI audit binary `db72a436…ddfa9`; empty private numeric
header cache; 48 kHz, 64-frame blocks, capacity 256 streaming voices, two
decoders, lazy 8 MiB head budget, all keys; zero played notes. No editor or
runtime was opened. Readiness is not first sound or editor-open latency. OS
page cache was not evicted. Load average was 13.34/18.51/22.31: **CONTENDED**,
so these values do not admit performance improvement or regression. Warm,
other ten libraries and matched before/after RSS are **not measured**.
`~/.cache/kontra-v1` is absent; historical v1 tables use different builds and
measurement scopes and are not a substitute for a fresh matched pair.

## Coordinator: exact deferred work

The user assigned all builds/corpus/measurements to the coordinator. The
following commands are instructions, **not commands this worker executed
after the stop**. The [835-file manifest](kontakt-load-manifest.tsv) and
[11 representatives](kontakt-load-representatives.tsv) are source-free.
Use the one merged build's `corpus-health` executable as `AUDIT_BIN`.

Run one bounded invocation for each 20-file slice, then release the heavy slot:

```bash
/home/derpcat/.cache/kontakto-heavy python3 tools/audit-load.py \
  docs/audit-2026-10-13/kontakt-load-manifest.tsv "$AUDIT_OUT/after" \
  "$AUDIT_BIN" loader 0 20
```

Repeat with `20 40`, `40 60`, …, `820 835`; exit 75 means resume that same
slice. Each invocation has a 235-second admission window and 130-second child
timeout, so the slot remains under five minutes. This examines all programs
of multis. Use `loader-stream` instead for full source/header/script binding;
set a private `PROBE_CACHE_HOME` to retain only numeric header metadata. Neither
mode renders samples or establishes native playback parity. Exclude Conflux
behavioral rows when ranking; retain its source/admission counts.

For each representative index `i=0..10`, use a new private numeric cache and
run these two invocations with the same cache, one slice per heavy call:

```bash
PROBE_CACHE_HOME="$AUDIT_OUT/headers-$i" /home/derpcat/.cache/kontakto-heavy \
  python3 tools/audit-load.py docs/audit-2026-10-13/kontakt-load-representatives.tsv \
  "$AUDIT_OUT/loads" "$AUDIT_BIN" loader-stream "$i" "$((i+1))" cold
PROBE_CACHE_HOME="$AUDIT_OUT/headers-$i" /home/derpcat/.cache/kontakto-heavy \
  python3 tools/audit-load.py docs/audit-2026-10-13/kontakt-load-representatives.tsv \
  "$AUDIT_OUT/loads" "$AUDIT_BIN" loader-stream "$i" "$((i+1))" warm
```

Measure in a granted quiet window and enable the existing activity observer
(`KONTRA_GATE_ACTIVITY=1`) to retain the contention verdict. This is an empty
product-header-cache comparison, not disk-cache eviction. Use the plugin load
probe from `tools/audit-load.py` mode `v2` if the acceptance metric is first
sound/editor RSS; keep its scope distinct from loader readiness. V1 comparison
needs an actual frozen reference binary before it can be admitted.

After merging, verify that valid omitted voice-group warnings disappear
because limits are bound, while the invalid-reference fixture still reports
its error. Check admitted group/zone counts against the preserved before
receipt, and resolve EQ/FX findings with the owning workers rather than
suppressing them.

## Ownership and topology handoff

The only reserved source edited is `crates/sampler-kontakt/src/library.rs`:
its voice-group assignment branch, an accurate override-mask comment, and a
four-line test-module hook. The user explicitly authorized this after the
restart. The coordinator must resolve its conflict with the concurrent thread.
Other reserved files and Conflux-specific behavior are untouched.

Native topology fact: all 128 voice groups exist independently of the
serialization override mask. A clear bit controls serialization omission;
it does not remove a shared runtime allocation domain. The v1 importer made
the same omission mistake and was not used as a parity oracle for this fix.
