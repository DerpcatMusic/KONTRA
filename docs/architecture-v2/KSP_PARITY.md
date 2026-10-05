# Kontakt 8.12 KSP completion map

Full Kontakt KSP parity is a required product gate. The current
[`ksp-8.12-note-subset-v0`](KSP_FRONTEND.md) compiler is intermediate work.
The target is the official manual identifying **Kontakt 8.12**, retrieved
2026-10-06. Its public URLs may change; per-page SHA-256 values preserve which
documentation was inspected. No licensed Kontakt differential run has occurred.

[`KSP_SURFACE.json`](KSP_SURFACE.json) indexes all 25 functional manual chapters,
288 sections and 1,605 distinct extracted interface identifiers. It includes the
engine, UI, MIDI objects, analysis and asset surfaces. These counts are **not** a
feature count or compatibility percentage. Operand rules, enum values, overloads,
callback restrictions and historical aliases still need section-level review.

Every entry defaults to `missing` implementation and `unverified` Kontakt fidelity.
Six named interfaces have explicit `partial_native_subset` overrides. A section's
partial status means some behavior exists; it does not promote all of its symbols.
Native services, legacy KONTRA tests and successful parsing are not vendor parity.

## Work allocation

| Work unit | Required surface | Native service / completion evidence |
| --- | --- | --- |
| KSP-LANGUAGE | All declarations, scalar/array types, operators, control flow, functions and diagnostics | Typed, bounded compilation and execution; numeric boundaries, precedence, coercions, string/array limits and malformed input fixtures |
| KSP-STATE | Script-instance globals, constants, polyphonic cells, built-ins and persistence | Explicit instance/note owners; wait/release retention, coherent recall, initialization ordering and PGS communication |
| KSP-EVENTS | Every callback, suppression, transformation, script-slot forwarding and MIDI/multi-script processing | Raw and stage-visible state separated from committed performance/expression state; stable equal-time order and synchronous read-after-write |
| KSP-NOTES | Generated notes, event handles/marks, waits/cancellation, note parameters, fades and release behavior | Native note/family/continuation ownership; no key-index lifetime guesses, stale retargeting or partial cleanup under pressure |
| KSP-ENGINE | Group/zone commands, engine parameters, source modes, loops, modulation, routing, filters and effects | Preserve script-visible source identities over compiled execution; exact parameter scopes/units, async mutation outcomes and DSP comparison evidence |
| KSP-UI | All widgets, UI callbacks, control parameters, automation, resources and keyboard presentation | New UI wired to owned native state; UI-closed execution/recall and sample-section integration, without the old runtime |
| KSP-ASSETS | Load/save, resource containers, user zones, MIDI objects, MIR/analysis and async completions | Control/worker ownership, bounded transfer, explicit completion identities, retained errors and off-audio destruction |
| KSP-REFERENCE | Every implemented section and cross-service behavior | Pinned Kontakt fixtures for traces, scheduling, state and audio; approximations or unsupported branches remain open gates |

All units belong to V2-14; underlying core tasks and V2-15/16 provide host/UI
execution. Imported source/profile semantics remain frontend concerns over shared
native services. No compatibility path to the old KONTRA engine is introduced.

## Immediate architectural obligations

- The controller callback covers CC, pitch bend and channel pressure; ignoring it
  must prevent downstream effects. A full-resolution native CC bank alone cannot
  stand in for KSP's virtual controller namespace or script-stage observation.
  [NI callbacks](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#on-controller),
  [NI controller suppression](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands#ignore_controller).
- Polyphonic variables belong to note events and remain available in their release
  callbacks. Callback-local scratch is insufficient. The native implementation
  now has [bounded note-owned integer cells](BEHAVIOR.md#note-owned-integer-state)
  shared across executions. KSP declarations, typed arithmetic and automatic release
  dispatch still require implementation.
  [NI variables](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/variables#polyphonic----polyphonic-integer-).
- Callback identity and event identity are separate; waits need retained callback
  context and cancellation. Controller callbacks must not require fabricated note
  owners merely to enter the behavior interpreter.
  [NI callback context](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/callbacks#general-information-101878).

Implement these native contracts before widening syntax in ways that would hide
missing ownership or event semantics. Then add executable frontend fixtures to the
same completion map. Full language and UI support remain mandatory.

## Reproduction and review

The 28 retrieved HTML pages (including introduction, history and additional
resources) and their fetch manifest are cached under ignored
`artifacts/ksp-8.12-reference/`. Only interface metadata is checked in, not manual
prose or examples. The offline stdlib extractor checks page hashes, known chapter
coverage, distinct section identities and representative interfaces before writing:

```sh
python3 tools/ksp_surface.py artifacts/ksp-8.12-reference artifacts/ksp-surface-check.json
cmp docs/architecture-v2/KSP_SURFACE.json artifacts/ksp-surface-check.json
```

The inventory is an initial source-review baseline. Implemented statuses and
reference evidence must be reviewed as work lands; regeneration from newer manual
bytes requires a deliberate target-version and coverage review. Open-source sampler
reviews in [REFERENCE_REVIEW.md](REFERENCE_REVIEW.md) supplement these requirements
but cannot establish Kontakt semantics.
