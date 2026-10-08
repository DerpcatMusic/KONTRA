# ni-file NIS preset reader patch

## Base and ownership

- Implementation checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/decipher-readers-v2`.
- Exact V2 base: `2fb8c926dd39bb7ac26a84d4806f42de14b6630e`, initially clean.
- Thread checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/t3code-80fe786b`.
- Exact thread base: `0cb7a8a0b4d43086596a64c77320caa1b26d6d98`.
- Source changes are only in V2 `vendor/ni-file/src/nis/schemas/kontakt.rs`,
  `vendor/ni-file/src/nis/items/preset.rs`, and
  `vendor/ni-file/src/kontakt/schemas/preset.rs`.
- This evidence file is in the thread checkout. No staging or commits were made.

## Behavior

`find_kontakt_preset_item` maps the existing `Option<Result<...>>` without
turning parser errors into absence. Both raw preset wrappers and
`extract_kontakt_preset` share `preset_chunk_data`, which propagates encryption,
subtree, and chunk errors with a `NIS preset payload` context. No key is supplied;
the existing EncryptionItem reader rejects protected payloads before subtree
decoding. No license, authentication, checksum, or key logic changed.

An absent EncryptionItem still returns `None`. A present malformed wrapper,
invalid subtree, or missing/invalid PresetChunkItem returns `Some(Err(...))`.
Instrument extraction keeps absent headers as `None` and propagates returned
header/schema errors. Raw bytes move out of PresetChunkItemProperties instead
of being cloned a second time.

KontaktPreset retains the Kon4–Kon7 NKI dispatch. Other application signatures
(including Kon8) and patch types return `Unsupported(KontaktChunks)`, preserving
chunk order, duplicate IDs, and bytes. Truncated chunk framing remains an error.
This fallback does not establish semantic support, playability, sample access,
or audio equivalence. NKM/NKB/NKP/NKG/NKZ/unknown patch schemas and Kon8 are not
newly decoded.

## Parent integration

At the base commit, `vendor/ni-file/src/nis/mod.rs` commented out `items` and
`schemas`. The parent enabled the item APIs and the Kontakt schema below.
Enabling all of `schemas` would also expose an old `schemas/repository.rs` that
imports the removed `crate::prelude`, so that unrelated module remains disabled:

```rust
pub mod items;
pub mod schemas {
    pub mod kontakt;
}
```

The parent also corrected the shared property-reader roots:
`nis/properties/bni_sound_preset.rs` now reports a missing inner frame instead
of unwrapping; `nis/properties/preset.rs` reports unsupported property prefixes;
`nis/properties/bni_sound_header.rs` validates magic and header version before
decoding its body. All three wrappers return ItemWrapError for incorrect item
types. Reads borrow existing bytes rather than clone them. One additional
regression covers missing properties, unknown authoring IDs, unsupported
prefixes, every truncated properties prefix, invalid headers and wrong types.
These are bounded fixes; other ni-file parsers still have unchecked paths.

## Evidence reviewed

Reviewed vendored README and format docs: NIS containers, AppSpecific, NKS,
Kontakt chunks, BPatchHeader, StructuredObject, and BProgram. Traced
ItemContainer find/find_data/find_item, EncryptionItem, SubtreeItem,
PresetChunkItemProperties, repository extraction callers, NIFile raw extraction,
KontaktPatch, and the Kon4–Kon7 conversions before editing.

The [ni-file upstream](https://github.com/monomadic/ni-file) returned HTTP 451
through the web tool. The referenced template repository was readable through
GitHub's API/raw endpoints at commit
`c6f309bae04a03967b94f54d81dc2050f827a1e8`:
[NISDFull](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NIS/NISDFull.tcl),
[NISD](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NIS/NISD.tcl),
[AppSpecific](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/NIS/AppSpecific.tcl),
[Chunk](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/Chunk.tcl),
[BPatchHeaderV42](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/BPatchHeaderV42.tcl),
[StructuredObject](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/StructuredObject.tcl), and
[BProgram](https://github.com/monomadic/hexfiend-templates/blob/c6f309bae04a03967b94f54d81dc2050f827a1e8/Kontakt/BProgram.tcl).
These support the layered/raw-chunk representation, not a Kon8-to-Kon7 alias.

## Validation for parent

No cargo/rustc/build/test/clippy was run by this agent. Formatting and
`git diff --check` passed. After wiring the NIS modules, run this ONE serialized
command from V2 (do not override Cargo target or rustc wrapper configuration):

```bash
cargo test --manifest-path vendor/ni-file/Cargo.toml --lib nis_readers
```

Expected five local regression tests, with no external fixture files:

- Both wrappers: absence, valid bytes, protected flag with deliberately invalid
  subtree, bad encryption/subtree versions, missing subtree/chunk, invalid inner
  item, and truncated chunk properties.
- Kontakt lookup/extraction: absent versus invalid versus valid properties,
  protected payload, malformed chunk framing, missing Program, opaque Kon8,
  and truncated header.
- Unsupported schemas: repeated/unknown raw chunks round-trip exactly across
  unknown signatures and all unsupported patch types; truncated framing errors.
- Known schemas: minimal structured Program and filename tables still select
  Kon4, Kon5, Kon6, and Kon7; incomplete known schemas return errors.

Parent validation passed: `cargo test --offline --manifest-path
vendor/ni-file/Cargo.toml --lib reader` ran 11 tests, including all five NIS
regressions and the three Kontakt reader regressions. The existing compatibility
suite with `--features serde --test compatibility` passed all 32 tests.
The activated legacy instrument API emits deprecation warnings; no errors
occurred. These authored cases verify the named boundaries, not every historical
preset or plugin playback. Later VoiceGroups follow-up edits require their own
serialized recheck.
