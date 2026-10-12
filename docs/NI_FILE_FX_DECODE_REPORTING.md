# Kontakt effect decode reporting

Implementation checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2`,
base `2fb8c926dd39bb7ac26a84d4806f42de14b6630e`. This is a working-tree change
to `crates/sampler-kontakt/src/effects.rs` and `library.rs`; the previous ni-file
reader changes are retained. Research date: 2026-10-08.

## Problem and resulting behavior

The importer previously used `.ok()?` and `if let Ok` while collecting effect
slots, instrument racks, buses and group inserts. A malformed occupied slot or
unreadable rack could disappear without an `Instrument::unsupported` entry.
An empty diagnostic list therefore hid some failures before DSP lowering.

The shared rack reader now reports each failed occupied slot to its caller.
The existing program/group translation paths add those errors to the existing
`Unsupported` list with feature `effect decoding`, reason `Unknown`, and the
decoder's error text. The source location keeps the rack owner and, where
available, the original slot number. No new report type or dependency was added.

| Source condition | Location/example | Result |
| --- | --- | --- |
| Clear slot flag | Any rack slot | Ordinary absence; no diagnostic |
| Wrong slot chunk ID or malformed BParFX framing/state | `instrument insert slot 2` | Located decode diagnostic; valid sibling slots still translate |
| Occupied slot without an effect child | `instrument insert slot 3` | Explicit missing-effect diagnostic |
| Malformed effect-object framing | `instrument insert slot 4` | Located decode diagnostic, rather than fabricated version 0/empty parameters |
| Rack revision rejected by the existing array reader | `instrument send` | Located decode diagnostic |
| Malformed bus or missing/unreadable bus rack | `bus 1` | Located decode diagnostic |
| Unreadable group private insert rack | `group 0 "" insert` | Located diagnostic; the remaining group semantics can still translate |
| Muted group | Group scope | Existing early return; no FX translation |

Rack/bus counters advance for every source record, including rejected records.
A damaged send rack therefore does not relabel the following main rack, and a
damaged bus does not renumber later buses. Occupied-slot indexes remain source
indexes after a failed sibling is removed from the executable slot list.

## Validation

Two authored regression cases exercise valid gain processing beside four kinds
of bad occupied slots, an unknown rack revision, damaged/missing bus racks,
group insert errors, muted-group behavior, retained coordinates and unchanged
raw source bodies. They reuse the existing authored chunk-framing helper.

Parent command, run only when the shared Rust build slot is idle:

```sh
cargo test --offline -p sampler-kontakt --lib
```

Validation status: **30 passed, 0 failed, 3 ignored surveys**. Both new regression
cases passed. The first build encountered incompatible cached IR metadata;
rebuilding the local `sampler-ir` dependency resolved that mismatch without an
IR/core source change. The shared target directory and sccache configuration
were retained. Existing deprecated ni-file API warnings remain.

## Boundaries

This exposes errors that the existing framing/state readers return. It does
not establish complete admission by version for every BParFX/module record,
every module's parameter law, or vendor-equivalent sound. A structurally valid
unknown effect still goes through the existing unsupported-module reporting.
Generic readers that accept a prefix of an unknown record need a separate
version/layout audit.

The raw source chunks remain available through the container APIs; these
diagnostics do not add original bytes to semantic IR. Translation neither edits
the input file nor supplies a lossless native serializer. Callers requiring
strict admission still need to inspect `Instrument::unsupported`, because the
core lowerer does not treat that list as a rejection policy.

This completes the decode-reporting item in the
[coverage audit](NI_FILE_COVERAGE_AUDIT.md#finite-implementation-backlog-ranked).
Program-tail and monolith research are separate tasks.
