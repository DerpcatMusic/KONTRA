# Kontakt source ownership

`sampler-kontakt` is a new, dependency-free source decoder. Its input is an
**already expanded Kontakt chunk payload**, not an NKI/NKM container. It does not
call the v1 importer or vendor decoder. The owning worker holds one immutable byte
buffer; all decoded records borrow that buffer and retain absolute source offsets.
No source buffer or vendor structure enters the audio runtime.

Implemented framing includes ordered chunks, structured objects, group/zone
record lists and script records with versions 0x50/0x60. Unknown chunk IDs,
duplicate IDs, opaque public/private fields, source group IDs and extension bytes
remain available without normalization. Chunk-bounded unstructured bodies retain
their version and public bytes too; this is the script layout authored by the v1
writer. Unknown script versions and unstructured array elements return explicit
errors when interpretation is requested. An unstructured array element has no
internal length: consuming the remainder would silently lose following records.
Its enclosing raw chunk remains accessible.

Script text, optional linked filenames, password hashes and persistent entries
remain exact bytes. Absent strings and empty strings differ. Old records without
a persistent table differ from an explicit empty table. A damaged table is an
error, never a silently reset script instance. Encoding, resource resolution,
bypass, edited-but-unapplied text, protection state, persistence application and
unknown extensions still require explicit semantic admission before execution.

Every read checks the remaining slice before advancing. Framing is validated
before iteration; group/zone and string counts have caller-supplied limits. These
views allocate nothing, including failure paths. They do not recursively inspect
arbitrary unknown data or claim every nested record has been interpreted. Limits
apply to the input/list being opened; total inspected bytes are bounded by the
owned input and the caller's traversal policy. The current CLI visits only root
programs and their immediate children.

## Source evidence and scope

Reviewed local v1 `src/import.rs` and the standalone vendored `ni-file` wire-format
documentation/readers at baseline `f68c975`: `StructuredObject`, `Program`,
`GroupList`, `ZoneList` and `BParScript`. These describe the existing byte layout,
not verified vendor playback semantics. The old path copies each nested body,
mixes source parameters with engine state, and drops malformed script persistence
tables. Its NKS zlib expansion also checks the output size only after expansion.
None of those implementation paths was imported into the new crate.

Authored byte fixtures verify exact unknown/duplicate preservation, offsets into
the original buffer, source group identities, malformed/truncated lengths, flags,
count limits and saved tables. Allocator instrumentation covers parsing and view
iteration. A separate fixture compiles saved UTF-8 KSP through `sampler-ksp`, drops
the source buffer, then executes a transposed event and checks native PCM under
the existing heap guard. This is evidence for the source/behavior ownership
boundary, **not whole-instrument admission or NKI playback**.

```sh
cargo test --locked -p sampler-kontakt
cargo run --locked -p sampler-native -- inspect-kontakt-chunks EXPANDED.bin
```

The inspection command is deliberately named for its input and reports playback
as unadmitted. NKS/NIS wrapper decoding with bounded decompression, complete
source schemas, asset resolution, group/DSP/control lowering, linked resources,
ordered script slots, saved state, UI generations and complete capability
diagnostics remain required. Falcon must contribute its own source model to the
same native services; it must not inherit a Kontakt-shaped runtime model.
