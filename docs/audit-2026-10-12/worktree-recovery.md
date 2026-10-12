# Worktree recovery, 2026-10-12

The historical branches contain valuable work, but their divergence from `main`
does not show that work is missing. The fetched baseline is
`11ca186dbeb1f1fc5714e36614ce92e979eb55f6` (PR #59). It has just 72 reachable
commits, while all local/remote refs together have 12,536: releases were repeatedly
squashed. In particular, **`fix/ui-mapping-controls@86effda4` has exactly the same
complete Git tree as this baseline**. Merging that branch would recover nothing.

The initial `/tmp/kontra-parity-audit-20261012/worktrees.json` snapshot recorded
**284 registered worktrees, 13 missing directories, 39 pre-existing dirty
worktrees, and 695 local branches**. The subsequent detailed scan counts 40 dirty
worktrees because it also observes this audit's current worktree
`feat/kontakt-falcon-parity`; its new files are separate from historical recovery.
Recursive untracked enumeration expands some original status groups into more
individual files (for example, the main v1 checkout has 238 entries).

Every branch and registered worktree is inventoried. Every historical dirty
worktree was inspected for its changed paths, source/test/probe purpose, and
operation markers; candidate changes received scoped source comparisons.
This is a recovery/source audit, **not a proof that every historical commit has
been incorporated, or a fresh audio/UI certification**. Graph counts and files
that differ from current source must not be labeled missing features.

## Machine-readable evidence

- [worktree-inventory.json](worktree-inventory.json): all branch/worktree identities,
  operation markers, per-file dirty/untracked hashes, baseline content comparisons,
  and normalized tip summaries.
- [branches.tsv](branches.tsv): all 695 branches, graph counts, whole-tree equality,
  last-commit scope equality, and subjects.
- [worktrees.tsv](worktrees.tsv): all 284 registrations, directory existence,
  individual status counts, and unresolved operations.
- [recovery-decisions.json](recovery-decisions.json): dispositions for all 39
  historical dirty checkouts and the current audit checkout.
- The full normalized per-tip path lists are compressed at
  `/tmp/kontra-parity-audit-20261012/recovery/worktree-inventory-full.json.gz`.
  They are reproducible with the tool below; the compact repository inventory
  retains the exact path counts without committing that redundant 7 MiB blob.

The directory scan covered `/home/derpcat/.t3/worktrees`, `.t3/scratch`,
`.codex/worktrees`, `/mnt/Windows11/DEV_WORKSPACE/kontra-worktrees`,
`/mnt/Windows11/DEV_PROJECTS/Repos/.worktrees`, and the main repo's
`.claude/worktrees`, at most six levels deep. It found **zero unregistered
checkouts belonging to this Git repository** and 63 checkouts of other
repositories. Missing registrations and directories were preserved. This bounded
scan cannot find already-deleted worktrees or prove that no checkout exists in
another location.

## What can be recovered

| Priority | Donor / exact baseline | Evidence and next action |
| --- | --- | --- |
| P0 | Main checkout `/mnt/Windows11/DEV_PROJECTS/Repos/KONTAKTO`, branch `docs/concise-readme-status@58c4c715` | 238 individual status entries; tracked delta is 75 files, 19,913 insertions and 6,631 deletions, plus substantial untracked code. This is ongoing v1 native-engine work: MIDI/MPE ownership, shared envelopes/LFOs, counted loops, HQ sampling/streaming, rack timelines and host expression. Preserve and extract independently bounded contracts/tests against v2. The branch's documentation name does not describe its working files; no successful current integration gate is recorded by this audit. |
| P1 | `fix-w13-product-parity`, branch `v2/w13-product-parity@cb728a1c` | Dirty `src/ui/chain.rs`, `src/ui/editor.rs`, `src/ui/v2_tests.rs`, and untracked `docs/audit-2026-10-08/w13-live-modulation-parity.md`. Adds watched-part/group key and velocity source bars and one primary amplitude-envelope assignment per distinct source. Baseline `chain::modulation` lacks these additions. Its handoff records two targeted passing regressions and says root no-run/push remained pending after W9 requested quiet. Port/test this scoped delta against current interfaces. |
| Unproven | `fix-w12-gate`, branch `v2/fix-w12-gate@23b8e94f` | The dirty delta adds only `native_always_keeps_its_compound_operator` to `crates/sampler-kontakt/src/keyswitch.rs`. Its `Always AND_NOT key` expectation lacks a native oracle. NI's manual describes Always as a no-operation row-removal/end marker; the group API excludes it. Current mode-0 skipping may be correct. Preserve the test as a hypothesis; establish malformed/interior-row native behavior before changing semantics. |
| P2 | `fix-widget-gate`, branch `v2/fix-widget-gate@235ac3eb` | Dirty `src/ui/widget_gate.rs` separates a widget's bound-control persistence from unrelated timer updates. `binding_keys`, `recall_counts`, and `recall_attribution_separates_bound_values_from_timer_updates` are absent from baseline. This is parity-measurement correctness work, not an audio fix. Validate current capture interfaces and report schema before porting. |
| Preserve | `integration7-source-draft`, branch `pi/integration7-source-draft@4fcb6cb7` | A cherry-pick of `d057af736d48cdbc0af0fca7293756b16a00a252` (`feat(ksp): admit owned waveform state and bounded UI projection`) is unresolved. `crates/sampler-ksp/src/lower.rs` has an unmerged index; 12 other paths are staged. Current waveform source already has successor work, and three staged files exactly equal baseline. Preserve this operation and review its residual edits; neither finish nor abort it as housekeeping. |
| Preserve | `ksp-rpn-deferred-fixture-successor-review`, detached `2a243bcf` | 2,299 staged deletions, no cherry-pick/merge/rebase marker. The deletions and sparse checkout configuration do not establish author intent. Treat this as a review/checkout anomaly requiring owner context; do not commit or replay its deletion set. |

Donor names above live under `/home/derpcat/.t3/worktrees/KONTAKTO/` unless an
absolute path is given. Uncommitted candidates have no commit to cherry-pick:
apply their reviewed file/hunk delta into an isolated integration branch,
retaining the original checkout unchanged. The saved tracked patches are at
`/tmp/kontra-parity-audit-20261012/dirty-diffs/`; the compact inventory also lists
untracked files and raw Git blob hashes. Those hashes prove captured content
identity, not successful compilation or desired behavior.

## Already integrated or superseded

`gpt/parity-luau@7c29f7cc` has current-equivalent sampler-parity and UVI parity
source/tests. Its ledger differs and the old branch lacks later unrelated
improvements. `gpt/script-view@96a09b37` would replace the current real KSP/Luau
report producer with an empty `Vec` and remove the two integration-seam tests:
the scoped baseline-to-branch delta has 4 additions and 92 deletions. Retain the
current producer. Neither whole branch should be merged for parity recovery.

The dirty NICNT candidate `v2/w12-vista-pacific-resources@68052b42` proposed
`read_index`/`read_member`. **Its behavior has a newer implementation already**:
current `ResourceContainer::open` calls `NIFileContainer::read_table(...).0`,
and current `read_item` provides bounded per-item reads while retaining duplicate
stored IDs. The absence of the old `read_index` name is not a missing feature.
Do not port the older API. The donor's own handoff also says it brings no
image-loading/playback gain for absent authored members.

Other concrete integration evidence:

- Five complete dirty ni-file files in `decipher-readers-v2` exactly match the
  baseline. Its `reader_regression_*` and FX-decode-error regression families
  are already in current source.
- The v1 `decipher-readers-v1` UVI binary-state decoder and bounded exact-framing
  test have moved into `crates/sampler-uvi/src/crypto.rs`.
- The detached `w15-baseline-5fc` deterministic Luau-work-budget tests and
  configured work limits are already current. Its CPU-witness example exactly
  matches baseline.
- Dirty `t3code-e50b9147` MUI renderer-fallback/backend-policy tests are already
  in the current vendors; its local Cargo dependency file exactly matches
  baseline. Review remaining arithmetic edits separately.
- Dirty `v2-simd-dsp` proposes older slot-filter/AR approximations. The donor
  explicitly says AR resonance and subtype behavior are unmeasured. Current
  AR and parameter routing have successors; do not replace them with this
  historical approximation.
- Nine untracked ni-file/architecture research documents in
  `t3code-80fe786b` exactly match their current `docs/re/` locations. Other
  research files need content comparison before copying; path absence alone
  does not establish loss.

Only nine local branch tips are ancestors of current main. However, 96 branch
tips have every last-commit changed path already equal to current baseline.
That proves their **last commit's resulting scope**, not the whole branch.
Sixteen graph-unmerged branch snapshots exactly match a reachable historical
release snapshot:

| Historical branch / tip | Identical release commit |
| --- | --- |
| `docs/kontra-roadmap-site@b5b814f2` | `970f4353` |
| `fix/latest-mui-20261004@1a1931dc` | `ac2adc98` |
| `fix/nightly-font-licenses-030344@625ece99` | `7cd326ee` |
| `fix/nightly-notes-limit-030306@b5cd9cee` | `81db45a3` |
| `fix/ui-mapping-controls@86effda4` | `11ca186d` |
| `integrate/core-v2@49af6438` | `76f626ee` |
| `parity/medium-next-batch-20261011@52005c2b` | `78e8948b` |
| `release/integrated-20261010@20a94f87` | `b0fd7385` |
| `v2/fix-uvi-discovery@72bb9767` | `646cde06` |
| `v2/load-speed@fb456b41` | `88ba32f1` |
| `v2/w0-next-native-batch@54d9a5c5` | `eb55fa78` |
| `v2/w0-ready-383@5498e631` | `83ad4d23` |
| `v2/w0-ready-next403@49af6438` | `76f626ee` |
| `v2/w0-sweep-382@f199980d` | `e53256e1` |
| `v2/w15-harp-shipping@f199980d` | `e53256e1` |
| `v2/w5-tester-callback-fuel@ce714f75` | `7cd326ee` |

## Every historical dirty checkout

This table accounts for all 39 from the initial snapshot. “Probe” means the
inspected additions measure or diagnose behavior; it does not mean the probe
was freshly run. “Candidate” remains unresolved until a focused integration
investigator validates it. Detailed paths, hashes, status codes and exact tip
IDs are in the machine inventory.

| Checkout | Disposition / inspected delta |
| --- | --- |
| Main `KONTAKTO` | Preserve large v1 native-engine work; staged and unstaged changes mixed. |
| `audit-load-v1` | Probe: numeric load/RSS timers and ignored publication/audio measurement. It forces an old cache hash in `build.rs`; keep isolated. |
| `audit-ui-v1-probe` | Probe: ignored idle/UI timing frames in the frozen v1 UI. |
| `audit-uvi-ui-probe` | Probe: untracked UVI resource-failure category test. |
| `decipher-readers-v1` | Partly integrated under v2 paths; binary-state framing is current. Review remaining monolith/subtree delta before porting. |
| `decipher-readers-v2` | Reader/FX error fixes largely current; five files byte-identical and regression families present. |
| `fix-dsp` | Probe: untracked frozen numeric EQ verifier. |
| `fix-load` | Test/probe: typed table-capture queue retention and idle/reopen measurement; current implementation needs comparison. |
| `fix-uvi-discovery` | Superseded older workstation-reader discovery/preferences flow; current native namespace implementation replaces that dependency. |
| `fix-viewmodel-base` | Probe: untracked ignored real editor/UI frame timing fixture. |
| `fix-w12-gate` | Unproved test assumption: `Always AND_NOT` semantics conflict with Always's documented no-operation/end-marker role. Requires native evidence. |
| `fix-w12-vista-pacific-resources` | Superseded NICNT index/member API; current `read_table`/`read_item` covers intent. |
| `fix-w13-product-parity` | Candidate UI port: primary-envelope inventory and live key/velocity bars; recorded no-run/push pending. |
| `fix-widget-gate` | Candidate measurement port: bound-control versus unrelated timer persistence attribution. |
| `gpt-decipher-dsp` | Probe: expanded metadata-only DSP census. |
| `gpt-format-fxmod` | Reader/research delta: extended primitive layouts and schema census; current typed-parameter readers require scoped comparison. |
| `gpt-format-legacy-baseline` | Probe: untracked supplemental legacy/header/version/resource census. |
| `gpt-uvi-ui-baseline` | Probe: widget-property inventory and untracked UI health example, explicitly survey-only. |
| `integration7-source-draft` | Preserve unresolved waveform cherry-pick and mixed staged paths. |
| `inv-load-speed` | Probe: ignored plugin-load timing test. |
| `inv-load-speed-v1` | Probe: corresponding frozen-v1 ignored plugin-load timing test. |
| `ksp-rpn-deferred-fixture-successor-review` | Preserve unexplained 2,299 staged deletions; do not replay. |
| `linux-native-library-openers` | Superseded native namespaces work is current; three donor files byte-identical and native-namespace tests present. |
| `pi-perf-acceptance-round5` | Generated nine `.cache` inputs/measurements; preserve external receipt context. |
| `t3code-80fe786b` | Research drafts; nine documents already copied byte-for-byte into `docs/re/`. Reconcile other research rather than duplicate the whole directory. |
| `t3code-e50b9147` | Renderer fallback now current; retain unresolved v1 arithmetic/native adapter deltas for separate review. |
| `v2-ksp-frontend` | Probe: untracked v1 KSP corpus compile/init example. |
| `v2-simd-dsp` | Superseded older slotted-filter/AR approximation; unmeasured AR laws should not replace newer kernels. |
| `v2-uvi-fix` | Scanner experiment: probe script-playable keys, change note-selection policy. It uses one mutable host across keys; validate state independence before using its results. |
| `w14-browser` | Tests: older browser redesign/query/banner regressions. Reconcile current UI structure first. |
| `w14-browser-main-attribution` | Probe: test-only browser work counters/allocation timing and ignored installed-catalog measurements. |
| `w15-baseline-5fc` | Deterministic Luau budgets now current; CPU-witness example byte-identical. Some thread instrumentation still needs comparison. |
| `w15-eq` | Probe: untracked synthetic-PCM/KSP piano-release routing example. |
| `w15-harp-shipping` | Probe: untracked ignored installed-Harp compaction fixture. |
| `w15-vista-harp` | Superseded/disconnected AR/phaser draft source and measurement tests. Its AR handoff explicitly says numerical admission pending. |
| `w5-conflux-red` | Regression families are current: marked-event selectors, menu getters and source-zone identity. |
| `w6-envmod` | Research-only v1 voice-evaluator contract; document explicitly says no timed runs/builds. |
| `w12-dsp-recount` | Probe: ignored installed native-filter metadata test. |
| `w14-bank-status-layout-394` | Test-only library-name/status layout assertion; compare current browser presentation before porting. |

## Reproduce and validate

```sh
python -m unittest discover -s tools -p test_worktree_audit.py -v
python tools/worktree_audit.py --repo . \
  --baseline 11ca186dbeb1f1fc5714e36614ce92e979eb55f6 \
  --output /tmp/kontra-recovery-new --jobs 4 \
  --scan-root /home/derpcat/.t3/worktrees \
  --scan-root /home/derpcat/.t3/scratch \
  --scan-root /home/derpcat/.codex/worktrees \
  --scan-root /mnt/Windows11/DEV_WORKSPACE/kontra-worktrees \
  --scan-root /mnt/Windows11/DEV_PROJECTS/Repos/.worktrees \
  --scan-root /mnt/Windows11/DEV_PROJECTS/Repos/KONTAKTO/.claude/worktrees
```

The two disposable-repository regression tests pass. They distinguish a
squash-equivalent branch from its dirty working files, enumerate untracked
names with spaces, and capture a real conflicting cherry-pick plus a missing
registered checkout without altering the observed status/content. The tool
uses `git --no-optional-locks`, reads only repository/worktree state, and writes
only its requested output directory. No existing worktree was reset, stashed,
restored, pruned, deleted, committed, or edited by this audit.

Graph ancestry, exact whole-tree equivalence, last-commit resulting-scope
equivalence, and raw working-file equality are deliberately separate. Neither
`git cherry` output nor an ancestry failure can decide what to merge after
squashed releases. Reasons above come from operation markers or explicit donor
handoffs where available; elsewhere the original owner's intent is unknown.
