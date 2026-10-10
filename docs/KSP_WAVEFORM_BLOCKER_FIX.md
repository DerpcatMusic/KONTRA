# Waveform blocker fix — source ready for fresh review

Parent: `d247e0550b7ce2019d837d7e5c391ee85bb8c687`, tree
`6807036133b3f681ad88904f5e0dd4e218201fae`.
Code and test sources: `1cdac230b0b51e28db5843ae1a45d02865228e0b`, tree
`13a66dd42ff4327553051ffdee8a032cb532dcb3`.
Historical runtime code: `d057af736d48cdbc0af0fca7293756b16a00a252`.

**SOURCE_READY_FOR_FRESH_REVIEW.** This is not independent review clearance,
Rust compilation, product execution, or a product PASS. All35 catalogued Rust
tests are **NOT_COMPILED / NOT_RUN**. Native behavior is **UNKNOWN**. The goal
of complete polished KONTAKTO using less CPU AND RAM than BOTH v1 and Kontakt
remains **UNACHIEVED**.

The independent source review requested changes for WF-R1 through WF-R4.
Its exact JSON SHA256 is
`cfad205e62c53994bf6fbd43d07be0ece0026114c728f3672cf989c0880388bc`.
This successor changes only the three reserved production spans and appends
synthetic regressions in the two existing waveform test targets. It does not
change the old request decoder or its original five tests.

## Fixes

- **WF-R1:** Model projection now uses the retained state address. Table entries
  have separate addresses per index. Highlight, Cursor, Flags and MIDI start
  each retain a scalar address. A setter removes earlier requests for the same
  owned address and appends the latest write. An identical immediate tail is
  already final and needs no change. A revisit after a different highlight must
  still replace it, even if the revisited value equals an older request. Other
  properties, table indices, widgets and service requests are not removed.
  Attachment retains the existing widget-only reset behavior. Runtime outbox
  ordering and attachment/intervening-effect boundaries are unchanged.
- **WF-R2:** Seed validates the complete recognized waveform request stream
  before writing any seed entry. UI identity must resolve by HIR name or ID to
  a resolved waveform. Attachment must have exactly three correctly typed
  operands, a positive source ID and captured source membership. A setter must
  follow that widget's attachment, have exactly four correctly typed operands,
  a recognized symbolic property, and an index admitted by the existing
  `Property::validate_index`. Even a malformed request later erased by reset
  rejects the initializer. Invalid recognized requests cannot fabricate an
  accepted Script/Store/UI snapshot. Unrelated service requests remain outside
  this narrow validation. Cache schema and restore code are unchanged. The
  existing compile error remains coarse (attachment/source wording at offset0);
  rejection is explicit, but this change does not add request-position metadata.
- **WF-R3:** Get/Set convert the stored attachment through checked `i32::try_from`,
  require a strictly positive value, then check the exact source marker. Wider
  values cannot wrap to another source. Zero and negative markers cannot admit
  nonpositive identities. Attach can still replace the unattached -1 sentinel
  after its own source and fixed-key admission, with the existing atomic reset.
- **WF-R4:** The public Waveform dispatch checks `args + count - 1` with
  `checked_add` before reading any operand or accessing state. Overflow returns
  `InvalidInput`, without mutation or effects. This check is only for Waveform;
  Instruction, register accounting, generic operations and RPN are unchanged.

No cache-schema, lowering, bind_modules, production part/epoch fence,
Instruction, behavior, stages, plan_programs, control, module-framework,
provider/painter, dependency, or global capacity change was made. Source
reservations and callers were reported before editing. Graft was queried first;
ambiguous/missing edges were checked by exact source search.

## Authored regression sources

The35-name catalog includes all23 previous names and byte-identical bodies,
plus12 new tests. New assertions cover:

- Public init getters and seeded scalar Highlight for 3→4→3, -1→3→-1 and repeated
  equal writes; Table remains per-index with final-write ordering.
- Public compiler/binder/Runtime getters before drain, then qualified headless
  projection after drain; unchanged-value revisit, clear, reattachment, other
  widget, table, cursor and unrelated-effect boundaries.
- Public cache restore/compile rejection for highlight -2/65536/i32::MAX, table
  -1/65536, noncanonical scalar index, wrong property/index/value/UI types,
  unknown symbol/name/ID, non-waveform UI, bad arity, missing/nonpositive source,
  no attachment, and a malformed request hidden by a later reset.
- Fresh/cached accepted highlight -1/0/65535, table0/65535 and scalar0, including
  imported names and multiple widgets with synthetic performance-view metadata.
- Raw public-core Get/Set reject stored 2^32+27, i64::MAX, zero, negative and
  unattached identities. Every prepared Store entry and the candidate new-write
  address remain unchanged; no effect is emitted. Exact positive i32::MAX
  Get/Set and reset from -1 remain admitted. Full-Store scalar overwrite remains
  admitted.
- All three actions accept the final complete u16 argument window and reject
  overflowing windows. The first malformed operand is i64::MAX, so an accidental
  read would produce a different fault. Destination, prepared Store and effects
  remain unchanged. Owned setup and snapshots are outside heap guards.
- Full-outbox matching setter-tail replacement admits a different final value;
  it does not increment dropped effects or fabricate a new outbox slot.

All heap assertions are **NOT_RUN**, not allocation evidence. Full-Store failure
fixtures query every prepared entry and the operation's only new-write address;
the public seam does not expose general Store enumeration. This is a source
expectation, not an executed full-memory snapshot comparison.

Future root-only combined-cycle filters, not execution requests:

| Package | Test target | Filter | Feature |
| --- | --- | --- | --- |
| sampler-core | waveform | waveform_ | existing union |
| sampler-ksp | waveform_runtime | waveform_ | existing union, including cache |
| sampler-ksp | waveform | waveform_ | existing union |
| sampler-ksp | ui_callbacks | waveform_headless_initial_seed_and_runtime_roundtrip_requirement | existing union |
| sampler-ksp | ui | typed_seed_meter_and_waveform_addresses_reach_ir | existing union |

Cache cases require the existing cache feature union in a later centrally
authorized cycle. No current batch append or per-lane build is requested.

## Immutable evidence and no-build checks

`evidence/waveform-blocker-fix-source-checks.json` pins parent/code trees,
full source Git blobs and inclusive spans, focused changed spans, complete
original source pins, all test names/body digests, the exact delta digest,
unchanged shared-file custody, immutable root inputs and primary documentation.
`tools/check-waveform-blockers.py` is a bounded independent source/model checker.
It reproduces both historical highlight counterexamples, checks625 finite
four-write highlight sequences, checks table/widget/effect/reset separation,
models cached schema rejection and accepted boundaries, and models exact source
conversion and all register-window edges. Assertions are tied to the observed
source mechanisms; they do not execute or interpret Rust/KSP. Model success is
not a product PASS or a compiler result.

`evidence/waveform-blocker-fix-syntax-checks.json` uses the already-installed
WASM grammar for13 Rust files. Twelve have no error/missing nodes. lib.rs retains
all four baseline cfg-destructuring error nodes, with exact baseline/current
types, text, positions and surrounding context digests. No introduced nodes
were found. The baseline file is not claimed wholly syntax-clean. Python AST
and Git whitespace checks also completed. No Rust typecheck was performed.

Five official NI archive/excerpt pairs were rehashed and the exact excerpts
read. The evidence records exact URLs, archive paths/digests and excerpt
paths/digests. They support signatures, property units, index relevance and
source/widget declarations only:

| Section | Excerpt SHA256 |
| --- | --- |
| set_ui_wf_property | `2a9471a493de93c180f0347fdbe5c79e5ac00ec8387c6dd2445a03f8ced165e4` |
| get_ui_wf_property | `1cb17d2563736d61695192c94857f00cae950997980124b57a6244f6886c5263` |
| attach_zone | `3d713aac8b97a88aa60682a50f445c8acae180563cb564c98549102638e9e599` |
| Waveform properties | `2a656b21901e4fcd4871a3fe335159bbebb3f40abe609ba9c0a1e8590771cdd3` |
| ui_waveform | `eed4be25411400bc47fe029b4aa04a4cff1588c08de47d84699285e98c1d713b` |

These are immutable primary documentation receipts, not native Kontakt or
Falcon runtime receipts. The getter excerpt's inconsistent three-operand setter
example remains archived; it is not an overload. Native highlight toggle/getter,
reattach, cursor edges, slice count, drag and painting laws remain **UNKNOWN**.
The scalar Highlight representation, clear -1 and reset defaults are existing
owned policies, not new native claims.

## Remaining gates

Fresh independent source review must precede integration acceptance. Root alone
can later authorize a combined build/typecheck/test cycle. Generic fabricated
Effect projection remains a trusted sink, not proof of source/state admission.
The qualified ScriptView helper has test callers only; production's real part,
epoch and instance fences are unchanged. No new production connection or full
stale-epoch proof is claimed.

Get still uses `behavior_bank_mut` and invalidates script revision despite being
a read. That nonblocking source performance observation is intentionally not
fixed here. Native parity, GUI painting and comparative CPU/RAM measurements
remain separate gates. No Cargo/rustc/clippy/build/test executable, host/native,
Wine, official reader, protected library, scheduler/server/install/publication,
nested agent or self-loop work occurred. No CARGO_TARGET_DIR/RUSTC_WRAPPER was
set. Old producer/reviewer/initial/main/frozen checkouts and receipts are
preserved. This lane stops at the source-ready handoff.
