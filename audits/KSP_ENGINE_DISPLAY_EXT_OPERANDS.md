# Runtime engine display Ext operand correction

Status: source READY for a following integration batch. Authored Rust tests are
**NOT_RUN_NOT_COMPILED**. Fresh independent source review is pending.
No own or native execution PASS is claimed.

## Bounded defect and change

Base: `2a243bcfa2262abb789265aa0951fd2a85854023`.

The runtime text lowerer formerly treated Ext like an ordinary read: address
arguments 0/1/2/3 and supplied value argument 4. This confused value, group, slot,
and generic while still producing plausible display text.

Ext now reuses the existing write-layout address helper: address arguments
0/2/3/4, supplied value argument 1. Ordinary display/read retain 0/1/2/3;
writes retain 0/2/3/4 with value argument 1. The helper itself is unchanged.
No new operation, runtime framework, parameter catalog, dependency, allocator,
host-fact provider, or DSP path is introduced.

Current callers: `Gen::append` display branch and `Gen::builtin` Set/GetEnginePar
branch in `crates/sampler-ksp/src/lower.rs`. Typed admission in
`builtins.rs` and `sema.rs` remains unchanged. All five Ext integer arguments
still pass parsing and semantic admission before lowering.

The consumer in `sampler-core/src/ops.rs` decodes parameter/group/slot/generic.
Its provided-value display branch formats that supplied value **without looking
up the binding**. Thus formatted text, or audio output, cannot prove address
identity. The emitted-op seam and separate bound read/write consumers are
necessary regression guards.

## Primary requirement evidence, not native execution

Official NI KSP engine-parameter commands:
https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands#get_engine_par_disp_ext--

Exact signature:
`get_engine_par_disp_ext(<parameter>, <value>, <group>, <slot>, <generic>)`.

Cached document SHA256:
`df2fe7192f32df7c4a738c713663bafdfec3630c2821fd9afe1887c7d5ed006d`.

Exact DOM excerpt SHA256:
`d1ed157ba48c640e622c65bf3c4481934e60f6c3e584c9ee5db1ca007ea156a4`.

Custody: root handoff `round16-selected-section-index.json`, archive
`official-docs/ksp-engine-commands.html`, and
`round16-selected-sections/ksp-engine-commands-get_engine_par_disp_ext--.txt`.
Retrieved 2026-10-10T03:03:28.593743+00:00. Unversioned primary documentation
supports operand identity only; version-bound native runtime remains UNKNOWN.

## Authored regression seams

- Library `lower::display_ext_tests`: exact constant operand layout; all five
  dynamic cells read once; supplied-value register outside the address range;
  nonnegative group/target and instrument -1/bus identities; ordinary display,
  read and write neighbors; public semantic admission of valid five-argument
  Ext calls and rejection of missing/extra/noninteger arguments.
- Integration target `strings`,
  `display_ext_supplied_constant_and_dynamic_values_use_existing_pan_volume_tune_formatter`:
  public compile/bind/Runtime/string cells, PAN/VOLUME/TUNE, distinct constant
  and computed dynamic supplied values versus the same existing formatter via
  normal display, not native expected strings. Complete synthetic bindings and
  actual read/write outcome addresses guard group/slot/generic identity.
  Mutated group, slot and generic reads must fail instead of hitting that binding.
  The existing `without_heap` fixture encloses trigger/render, not compilation.

No Rust compiler, Cargo, clippy, test executable, host, native reader, Wine,
protected library, performance probe, server, scheduler, install, or publication
was run. No nested agents were used.

## Remaining gaps

Init display still returns empty text in `eval.rs`; its formatter/host-facts
dependency is separate and unchanged. Complete display parity is not claimed.
Native strings, localization, Unicode, quantization, invalid-address/error laws,
range behavior, switches and stepped parameters remain UNKNOWN.

Complete polished KONTAKTO with CPU **and** RAM below **both** v1 and Kontakt is
**UNACHIEVED**. This operand correction provides no comparative performance or
memory evidence. Root must perform fresh independent review and its sole
combined integration validation in a following batch; do not append this to
the current batch.
