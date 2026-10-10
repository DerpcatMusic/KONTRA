# KSP waveform operands: source-ready initial fix, runtime gate open

Base: `2a243bcfa2262abb789265aa0951fd2a85854023`.
Code/tests: `a09157e92151250a1e9dcaf88ffa16c0be6a0570`.

Initial snapshot decoding is fixed in source. All Rust tests are **NOT_RUN**.
Init getter and runtime attach/set/get are **UNIMPLEMENTED**. Native Kontakt
runtime behavior is **UNKNOWN**. CPU AND RAM below BOTH v1 and Kontakt remain
**UNACHIEVED**. This lane performed no compilation or product execution.

## Vendor requirements and exact evidence

The root inputs are `handoff/round11-zone-ui-consumer-requirements.{md,json}`
under `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff`.
Their byte digests, five selected official excerpt/archive digests, immutable
source Git SHAs/blobs/full inclusive line spans, and test-source pins are in
`docs/evidence/waveform-operands-source-checks.json`. No native receipt exists
for this lane. These are archived primary NI documentation requirements, not
Kontakt 8.13.1 execution receipts or a Falcon behavior claim.

Official `user-interface-commands` archive SHA256:
`ad269b9e26a32e9f1a8aea9b00d8e199af43551adb3b47cf71b93a7234e09d5e`.

| Primary NI section | Exact excerpt SHA256 | Requirement |
| --- | --- | --- |
| [set_ui_wf_property](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#set_ui_wf_property--) | `2a9471a493de93c180f0347fdbe5c79e5ac00ec8387c6dd2445a03f8ced165e4` | Setter has four operands: variable, property, index, value. |
| [get_ui_wf_property](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#get_ui_wf_property--) | `1cb17d2563736d61695192c94857f00cae950997980124b57a6244f6886c5263` | Getter has three operands: variable, property, index. |
| [attach_zone](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-commands#attach_zone--) | `3d713aac8b97a88aa60682a50f445c8acae180563cb564c98549102638e9e599` | Variable and source zone ID; flags combine bitwise. TABLE/MIDI_DRAG require SLICES. |
| [Waveform properties](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/control-parameters#ui_waveform-731967) | `2a656b21901e4fcd4871a3fe335159bbebb3f40abe609ba9c0a1e8590771cdd3` | Cursor is microseconds; table value and highlight target a slice index. FLAGS and MIDI start use value. |
| [ui_waveform declaration](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/user-interface-controls#ui_waveform) | `eed4be25411400bc47fe029b4aa04a4cff1588c08de47d84699285e98c1d713b` | Waveform variable names the widget; dimensions are grid units. |

The getter section's example uses an erroneous three-operand setter. Its exact
archived excerpt is retained in the root handoff. It does **not** define an extra
overload. The setter's signature and separate cursor example require index 0
followed by the cursor value.

## Initial source change and reservations

Before edits, this lane reserved base `ui.rs:715–778`, `tests/ui.rs:330–377`,
new waveform-only test/data/docs files, and an append after
`tests/ui_callbacks.rs:206`. Graft callers/query were inspected first; indexed
ambiguities were checked with source search. The worktree is isolated at
`/home/derpcat/.t3/worktrees/KONTAKTO/waveform-operands-round11`.
No old worker, frozen checkout, receipt, root integration file, or batch list was
modified. `sampler-ksp/src/lib.rs` and its RPN callback binding region are untouched.

`eval::request` already preserves operands. `ui::waveform` now decodes exact
four-element setter and three-element attachment shapes. Malformed types or
arity are ignored without replacing existing snapshot state. Properties remain
symbolic names; widget matching still uses original numeric UI ID or variable
name. The existing `sampler_ui_ir::Waveform` fields and bounded storage are reused.

The old shared test used reversed operands. It now uses cursor `(0,12000)` and
TABLE_VAL `(3,77)`, expects `[0,0,0,77]`, and checks no entry at 77. Additional
authored tests distinguish flags `(0,11)`, MIDI start `(0,65)`, highlight `(3,1)`,
separate widget/source-zone IDs, invalid slice indices, large values that must
not determine allocation size, unknown selectors, malformed requests, and the
invalid three-operand setter.

Highlight was already using the third operand as the slice index. Keep that
behavior: selecting index 3 is not selecting value 1. NI's selected text does
not specify fourth-operand toggle semantics or highlight getter representation.
The initial `cursor=0`, `midi=60`, MIDI clamp 0..127, table bound 65536, negative
highlight clearing, and reattach snapshot reset remain **owned policies**, not
native-certified laws. No additional slice-count or zone validation is invented.

## Actual consumer slice at code SHA a09157e9

All spans below are pinned by full SHA/blob/span digest in the evidence JSON.
Only the initial decoder is changed; the remaining paths were inspected.

| Consumer | Exact path/span | Current behavior |
| --- | --- | --- |
| Signatures | `crates/sampler-ksp/src/builtins.rs:257–269` | Correct attachment, setter, getter arity. |
| Init recorder | `crates/sampler-ksp/src/eval.rs:897–920` | Preserves argument order and symbolic property. |
| Initial waveform getter | `crates/sampler-ksp/src/eval.rs:1665–1690` | Falls to zero, not waveform state readback. |
| Initial IR decoder | `crates/sampler-ksp/src/ui.rs:715–782` | Fixed ordered snapshot projection. |
| Runtime variable lowering | `crates/sampler-ksp/src/lower.rs:1351–1365` | UI variable lowers to UI ID, not widget variable value. |
| Runtime effect operands | `crates/sampler-ksp/src/lower.rs:1404–1493` | Ordered numeric operands, owning plan/instance carried later. |
| Runtime dispatch | `crates/sampler-ksp/src/lower.rs:2730–2775` | Setter/attach generic effects; getter unsupported fallback zero. |
| UI sink | `crates/sampler-ksp/src/lib.rs:296–518` | No waveform setter/attachment branch; these effects return false. |
| Script view routing | `src/sound/mod.rs:259–275` | Routes effect to selected script view; not authoritative script state. |
| Publication fence | `src/plugin.rs:1727–1795` | Part/epoch admitted before applying view effect and again before publication. |
| Peak provider | `src/sound/waveform.rs:26–95`, `src/plugin.rs:666–787` | Physical source identity/plan and epoch-qualified data already exist. |
| Painter | `src/ui/ir_view.rs:1610–1616`, `src/ui/render_art.rs:14–85` | Peak envelope and cursor/duration; not slice/table/highlight or MIDI drag implementation. |

No drawing changes or native interactive parity claims are made.

## Concrete next dependency: coordinated owned state, not another GUI sink

Reuse these existing seams, with exact reservations coordinated before edits:

1. **Init evaluation and seed.** `eval.rs:897–920` and `1665–1690`, plus
   `compile_initialized_inner`'s state assembly in `lib.rs:1368–1455` (not
   `bind_modules` receiving-callback code). A validated initial property model
   must answer getters during init, then seed the same per-instance live state.
   Post-init replay alone cannot answer an earlier init getter correctly.
2. **Runtime same-state access.** `lower.rs:2730–2775` and the existing
   `set_control_par` precedent at `3684–3734` show where to evaluate operands
   once, validate widget/property/index, write state, and read state before
   optional publication. Use the existing Builtin identities/symbol namespace
   and Waveform data rather than inferred vendor numeric property ordinals.
3. **Admission/capacity dependency.** `sampler-core/src/ops.rs:572–639` and
   `1530–1541` provide instance/plan-owned `Op::Store`. This is **not** a complete
   waveform service: store insertion is ordered/O(n), default extra headroom is
   4096, and full insertion only increments `dropped`. There is no setter success
   result to condition a following effect. A runtime table allowed up to 65536
   indices cannot blindly assume that headroom; attach reset also needs a
   coherent policy, not an unbounded request log. Select bounded admission and
   publish only accepted state changes. Otherwise a dropped store write and an
   accepted GUI effect disagree. Do not add a new large shared framework or
   preallocate 65536 cells per widget without CPU/RAM evidence.
4. **Projection is a mirror.** Add validated/coalesced waveform projection to
   `apply_ui_effect` only after state admission exists. Retain source UI ID,
   script instance, source zone ID, and the current plan/epoch fences. Never make
   readback depend on GUI-open status or effect draining. Do not implement zones,
   meters, filesystem services, RPN, or slice/drag painting in this slice.

This is a next-lane scope, **not implemented here**. Existing stores can be
reused, but complete attachment/state validation and capacity-consistent
projection cross shared modules. This lane stopped at the complete initial fix
instead of committing a partial runtime effect handler or a speculative service
architecture. Highlight value/getter law, reattach resets, cursor edge behavior,
source slice-count bounds, and MIDI drag timing/content remain native-gated.

## Tests and no-build checks

The source-only test catalog is:

- `sampler-ksp / ui / typed_seed_meter_and_waveform_addresses_reach_ir` — corrected.
- `sampler-ksp / waveform / waveform_` — five enabled source regressions plus
  the ignored `waveform_init_getter_roundtrip_requirement` gate.
- `sampler-ksp / ui_callbacks /
  waveform_headless_initial_seed_and_runtime_roundtrip_requirement` — ignored
  runtime gate. It reuses existing runtime/context helpers and requires initial
  seed readback followed by same-callback cursor/flags/table/MIDI setter/getter
  readback, without creating a ScriptView or draining effects.

**All authored/changed Rust tests: NOT_RUN.** Ignored gates must not be counted as
passing tests. The runtime table-at-77 default-zero assertion is an owned fixture
policy, not a certified vendor invalid-index return law. Future instance-isolation,
capacity failure and stale-plan/epoch replacement checks are separate requirements
in `docs/evidence/waveform-operands-cases.json`, not already-authored Rust tests.

Checks performed (no build, install, product binary, or test executable):

```text
python3 tools/check-waveform-operands.py --handoff <root-handoff>
node tools/check-waveform-syntax.mjs <existing-graft-node_modules>
git diff --check
```

The Python check independently applies assignment selectors extracted from source
to eight documented/owned cases. It catches seven baseline mismatches, including
`{77:3}` versus `{3:77}`, and checks all eight corrected cases. It verifies five
cached NI archives/excerpts and pins source/test bytes. This is **not execution
of the decoder**, Rust type checking, or integration behavior evidence.

The Node check uses an already-installed WASM Rust grammar and reports zero
syntax error/missing nodes in four Rust files. Grammar digest and exact checked
file digests are in `docs/evidence/waveform-operands-syntax-checks.json`.
It does not check types, APIs, KSP semantics, or native behavior. Root integration
alone owns any later combined build/test; this lane neither appended a batch nor
requested a per-lane build.
