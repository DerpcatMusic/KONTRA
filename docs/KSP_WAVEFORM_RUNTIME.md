# Waveform state: source-ready admitted headless slice

Exact base: `d1fb71954431d858f4437d1fb8492db820d68f18`.
Implementation/tests: `d057af736d48cdbc0af0fca7293756b16a00a252`.
Predecessor order: `a09157e92151250a1e9dcaf88ffa16c0be6a0570` →
`d1fb71954431d858f4437d1fb8492db820d68f18`, based on frozen
`2a243bcfa2262abb789265aa0951fd2a85854023`.

**Status:** the admitted init/runtime getter, setter, attachment and headless
projection path is implemented in source. It is not execution-validated.
All authored Rust tests and Rust type checks are **NOT_RUN**. Native Kontakt
behavior is **UNKNOWN**. CPU AND RAM below BOTH v1 and Kontakt remain
**UNACHIEVED**. A fresh independent source review is required before any
root-authorized combined integration cycle.

## Primary requirements, not native receipts

The five immutable NI archive/excerpt pairs are verified again in
`evidence/waveform-runtime-source-checks.json`. Exact excerpt SHA256:

| NI primary section | Exact excerpt SHA256 | Requirement used |
| --- | --- | --- |
| set_ui_wf_property | `2a9471a493de93c180f0347fdbe5c79e5ac00ec8387c6dd2445a03f8ced165e4` | Four operands: waveform variable, property, index, value. |
| get_ui_wf_property | `1cb17d2563736d61695192c94857f00cae950997980124b57a6244f6886c5263` | Three operands: waveform variable, property, index. |
| attach_zone | `3d713aac8b97a88aa60682a50f445c8acae180563cb564c98549102638e9e599` | Variable and source zone ID; bitwise flags; TABLE/MIDI_DRAG display needs SLICES. |
| Waveform properties | `2a656b21901e4fcd4871a3fe335159bbebb3f40abe609ba9c0a1e8590771cdd3` | Cursor is microseconds; table/highlight use indexed slices; flags and MIDI start use value. |
| ui_waveform declaration | `eed4be25411400bc47fe029b4aa04a4cff1588c08de47d84699285e98c1d713b` | Original waveform variable names the widget; dimensions are grid units. |

The evidence JSON retains exact primary URLs, archive digests, owned excerpt
paths, applicability, source commit, Git blobs, inclusive spans and byte
SHA256. The main UI-command archive SHA256 is
`ad269b9e26a32e9f1a8aea9b00d8e199af43551adb3b47cf71b93a7234e09d5e`.
These cached unversioned NI pages are an equivalent immutable documentation
receipt, not native execution evidence. No native target was opened, and no
native execution receipt is claimed. No Falcon behavior is claimed.

The adjacent getter example's erroneous three-operand setter is still archived
as an inconsistency. It does not define another overload. The original five
initial-decoder regression bodies/assertions remain byte-identical to d1fb.
Only their fixture helper now supplies explicit synthetic physical sources
27 and 91. The documented-order and wrong-order negative cases are preserved.

## Accepted-state contract

1. **Identity and ownership.** Only resolved waveform UI definitions are seeded.
   Each instance's existing Store owns five fixed keys per waveform, keyed by
   its original HIR UI ID. Mutable script cells use a different bank. Other UI
   properties, listener, menu and shared-PGS tags are unchanged. Physical source
   IDs are admitted from that initializer's existing `Environment.zones` map;
   they are not dense runtime region indices or Mapping selection indices.
2. **Typed symbolic properties.** `Property::from_name` maps exact NI names to
   owned typed addresses. A per-instance seeded symbol map preserves the HIR's
   interned constant values. The internal enum ordinals and tag numbers are not
   guessed NI numeric constants. A variable containing an interned property is
   supported; unknown selectors are rejected.
3. **Init state and readback.** Init retains accepted, coalesced requests in its
   existing model. `ui::waveform_requests(model, id, HIR-name)` reconstructs the
   same snapshot off audio. Init does not yet have assembled widgets, so the
   small existing `waveform(model,id)` wrapper supplies an assembled name later.
   Its request-decoder body is unchanged. Getter results are available at the
   point of the init call, not just after a post-init replay. Compile then seeds
   that accepted state into the instance Store. No new Initial field, cache
   schema or duplicate authoritative init model is added.
4. **Runtime state.** Lowering evaluates each operand once and emits the narrow
   `Op::Waveform`. Getter reads its owner's live Store, even with no UI, without
   draining effects. Setter and attachment first validate UI/source/property/
   index and all five preseeded keys. Store capacity and outbox admission are
   checked before mutation. Valid readback does not depend on a GUI-open state.
5. **Failure policy.** Invalid init calls warn and are ignored. Invalid runtime
   calls fault with `InvalidInput`; capacity failures fault with `Capacity`.
   The core's signed32 operand conversion also retains its existing
   `ArithmeticOverflow` fault for a malformed wider integer. Neither changes
   waveform state nor publishes a rejected operation. This is
   per-operation atomicity, not callback-wide rollback: earlier accepted calls
   and the host control interaction retain their existing behavior. Generic
   `Op::Store` still silently counts/drops full-capacity insertion and leaves a
   missing read's destination unchanged; its execution block is byte-identical.
6. **Attachment/reset policy.** An accepted attachment replaces only its own
   source, uses supplied flags, sets cursor to0, MIDI start to60, highlight to-1,
   and clears that widget's sparse table. All five scalar keys remain present.
   Reset therefore works at full Store capacity without allocation. These are
   explicit owned policies retained from the decoder, not native reattach laws.
7. **Projection.** A state-admitted operation emits an owner-qualified effect.
   Only an immediately consecutive setter with the same plan, instance, service,
   UI, property and index can replace the outbox tail. Attachment and every
   intervening command are ordering boundaries. A full outbox rejects before
   state mutation unless this tail replacement is possible. The off-audio model
   mirror coalesces waveform properties and removes only that widget's waveform
   requests on reset. It never removes unrelated service requests.
8. **Fences.** `ScriptView::apply_ui_effect_for` rejects mismatched actual versus
   expected PlanId/instance before projection. The caller must supply the plan
   admitted by the real part/epoch fence. This helper is not an epoch oracle or
   proof that caller-supplied identities are current. Production's existing part,
   epoch, selected-instance routing and publication fences remain unchanged.
   Retained old callbacks keep their old plan/instance state; their effects are
   rejected when projected with the new plan's qualification.

The generic unqualified `apply_ui_effect` API remains the existing trusted sink.
It is not an admission service for fabricated effects. Its waveform branch
checks shape, widget kind, property symbol and index; source membership and
successful state admission are proven by the typed producer before emission.
Host routing and epoch qualification must precede use of that trusted sink.

## Bounds and allocation

- Table/highlight indices use the existing decoder's owned 65536 annotation
  bound. This is not a known native slice count or a Store capacity guarantee.
- Scalar index must be the canonical0 in this owned state service. NI describes
  index as relevant only to table/highlight; native nonzero-scalar-index behavior
  remains unknown. Highlight -1 retains the owned clear policy. Its getter returns
  the retained index or-1; fourth-operand toggle/getter laws remain native-gated.
- MIDI start clamps to0..127 as before. Cursor accepts signed32 microseconds.
  Other cursor boundary laws and flags' rendering interactions remain unknown.
- The existing `store.len() + 4096` capacity recipe is unchanged. Legacy seed
  deduplication can affect actual free entries; no global capacity is widened.
  Initial nonzero annotations are seeded sparsely; missing annotations read0.
  Runtime zero writes also need admission for a missing key. No65536-cell-per-
  widget preallocation, new option, dependency or generic service framework exists.
- Runtime reads/admission perform a fixed number of O(log S) sorted Store
  lookups. New-key insertion is O(S) bounded shifting. Reset is O(S) retain;
  outbox-tail coalescing is O(1). S is the prepared instance Store capacity.
- Store/outbox capacity is allocated off audio. Runtime validation, mutation,
  reset and effects use existing reserved buffers and do not allocate. Heap
  guard assertions are authored but **NOT_RUN**; this is a source invariant,
  not a measured real-time receipt.
- Init reconstruction and UI projection can allocate off audio. Decoder IR
  tables retain their existing grow-to-index representation and limit. This
  lane makes no CPU/RSS benchmark or comparative performance claim.

## Pinned actual consumer spans

At exact code SHA d057af73, these full spans and their Git blobs/digests are in
`evidence/waveform-runtime-source-checks.json`:

| Path | Inclusive span | Responsibility |
| --- | --- | --- |
| sampler-core/src/waveform.rs |1–139| Typed admission and commit; owned keys/policies. |
| sampler-core/src/ops.rs |621–639| Bounded Store availability/reset helpers; unchanged set body. |
| sampler-core/src/ops.rs |1756–1791| Admission → outbox preflight → state commit → accepted/coalesced effect. |
| sampler-ksp/src/eval.rs |921–964| Once-evaluated init operands, rejection diagnostics and getter. |
| sampler-ksp/src/lower.rs |1412–1429| Once-evaluated runtime operands and typed Op. |
| sampler-ksp/src/waveform.rs |1–85| Init/seed and off-audio coalesced model projection. |
| sampler-ksp/src/ui.rs |715–787| Small wrapper extraction; unchanged ordered request decoder. |
| sampler-ksp/src/lib.rs |1406–1408| Accepted initial-state Store seeding. |
| sampler-ksp/src/lib.rs |322–344| Headless UI mirror consumer. |
| sampler-ksp/src/lib.rs |290–298| Explicit PlanId/instance projection qualification. |
| src/sound/mod.rs |259–275| Unchanged selected-script view routing. |
| src/plugin.rs |1727–1795| Unchanged part/epoch intake and publication fences. |

The `bind_modules` callback region is byte-identical to d1fb, body SHA256
`6d31ddb97065706c719b69a868fd1080058901f6d8faaf3fd13ae67658c8e034`.
No Instruction variant or behavior.rs change exists. stages.rs,
plan_programs.rs, prepare.rs, controller_event.rs, init_cache.rs, RPN tests,
zone/meter/filesystem/sample/loop services, plugin/provider/painter and all
frozen checkouts/receipts are untouched. Baseline source reservations and
Graft callers/query plus ambiguity fallback were reported before widening.

## Authored test filters — root combined cycle only

All23 relevant Rust test sources are **NOT_RUN** (22 waveform-prefixed tests
plus the adjacent corrected typed-seed UI test). Both old ignored init/runtime
gates are enabled because their expected path now exists in source; enabling
is not a PASS receipt.

| Package | Target | Filter | Source coverage |
| --- | --- | --- | --- |
| sampler-ksp | ui | typed_seed_meter_and_waveform_addresses_reach_ir | Original corrected order/assertions, explicit physical-source fixture. |
| sampler-ksp | waveform | waveform_ | Original5 byte-identical bodies + init getter roundtrip. |
| sampler-ksp | ui_callbacks | waveform_headless_initial_seed_and_runtime_roundtrip_requirement | Initial Store seed + same-callback setters/getters with no UI drain. |
| sampler-ksp | waveform_runtime | waveform_ |11 public compiler/binder/Runtime fixtures: once-evaluation/dynamic symbols, accepted projection, malformed identities/indices, Store/outbox pressure, reset/order, instance/widget/cell isolation, retained old plan/stale identity, imported placeholder and cache. Cache case is feature-gated. |
| sampler-core | waveform | waveform_ |4 public Runtime fixtures: zero-headroom fault, missing-preseed atomicity, unchanged legacy Store drop/read behavior, Instruction-size bound. |

Fixtures use owned synthetic source metadata; no protected library, official
reader, native host, Wine or UI interaction runs occur. Heap allocation for
control IDs, views, source construction and UI draining is outside guard scopes.
Cache hit/miss retains the captured environment; a synthetic cached request
with its captured physical source removed is rejected during seed compilation.

## No-build checks and limitations

`evidence/waveform-runtime-source-checks.json`, SHA256
`867cba4b753fcb001da8413b1f3b63ff8a273a5de21d34a42e093ebb610fd2d8`:
10 independent finite transaction-model cases +3 qualification cases pass;
5 exact primary archive/excerpt pairs verified; original5 function bodies and
request decoder body preserved; generic Store code and reserved files pinned.
This is not interpretation or execution of Rust/KSP and not type checking.

`evidence/waveform-runtime-syntax-checks.json`, SHA256
`4fc944ef3c5f57bc36dccec4fae356f983658c40257059fe282b2c12f8cfab34`:
installed WASM grammar checks13 Rust files.12 have no error/missing nodes.
lib.rs has the same4 pre-existing grammar errors as exact d1fb, at its existing
`#[cfg]` destructuring. Exact baseline/current types, text, locations and
surrounding-byte digests are retained; zero introduced errors. The file is
not falsely reported as wholly syntax-clean. Python AST and `git diff --check`
also pass. None of these checks compile Rust, resolve APIs or exercise Runtime.

Remaining gates: fresh independent review, then root-only authorized combined
build/typecheck/test (including the cache feature union). Runtime behavior is
not validated until those run. Native highlight/toggle/reset/cursor/slice laws,
MIDI drag, slice/table/highlight painting and native interactive parity are
unknown/unimplemented beyond this owned state. Existing generic waveform
colors/readback/paint are preserved; slice color painting, WF_VIS_MODE and
BG_ALPHA, meter peak/overload, loop/sample APIs, zone mutation/registry expansion,
async permissions and Mapping selection-index domains remain separate gates.
The existing peak/cursor painter and real plan/epoch provider are not changed.

No per-lane build is requested, no batch is appended, no nested agent or
self-loop is created. Source work stops after READY handoff to root.
