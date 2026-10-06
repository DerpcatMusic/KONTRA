# Kontakt source ownership

`sampler-kontakt` is a new, dependency-free source decoder. `Chunks::parse` takes an
**already expanded Kontakt chunk payload**. `Nks42` separately opens the bounded
non-monolithic NKS 4.2 container profile; `nis::Item` opens modern NIS framing.
None of these decoders calls
the v1 importer or vendor decoder. The owning worker holds one immutable byte
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

The inspection commands are deliberately named for their input and report playback
as unadmitted. Remaining container profiles/wrappers, complete
source schemas, asset resolution, group/DSP/control lowering, linked resources,
ordered script slots, saved state, UI generations and complete capability
diagnostics remain required. Falcon must contribute its own source model to the
same native services; it must not inherit a Kontakt-shaped runtime model.

## Bounded NKS 4.2 decoding

`Nks42::parse` borrows the 222-byte header, compressed payload and metadata footer.
The admitted layout has format word 0x0110, the reviewed outer/header/footer magic
values, and a zero monolith field. Unsupported versions and monoliths fail
explicitly. Header flags, version metadata, checksums and the footer remain raw;
checksum verification and full metadata interpretation are still required before
production instrument admission. No decrypted/encrypted access paths are implied.

`expand` checks the caller's output limit and walks every FastLZ token before
allocating. This pass checks source bounds, backward-reference distances, output
growth and exact declared length. The second pass fills one reserved destination;
forward byte copying preserves overlapping runs. Malformed streams and false huge
output claims allocate nothing. Decompression is a worker operation, not audio DSP.

The existing dependencies were unsuitable for this boundary: `fastlz` 0.1.0's C
implementation reads match extensions/distances without checking the compressed
input end; `lz77` 0.1.0 swallows control-read failures and has no expansion budget.
The new decoder uses safe Rust slices and no codec dependency.

Wire rules were reviewed against [upstream FastLZ](https://github.com/ariya/FastLZ/blob/b1342dabcf5257ab303743c9332fe75e9147a011/fastlz.c),
with its MIT notice retained in the crate. Checked-in level-1/level-2 streams were
generated by `fastlz_compress_level` at that pin; their source recipe and expected
bytes are independent Rust test code. The fixture manifest records commit, sizes
and SHA-256 hashes. They exercise short/extended lengths, overlapping runs and a
12,000-byte dictionary distance; authored malformed fixtures exercise truncation,
invalid references/levels and allocation rejection. A CLI fixture verifies NKS
and expanded-input inspection produce the same source report without editing input.

```sh
cargo run --locked -p sampler-native -- inspect-kontakt-nks INPUT.nki
```

This is container/source decoding evidence, not Kontakt audio/UI fidelity. Remaining
NIS profiles, old NKS zlib profiles, monoliths, assets and semantic lowering are
still open. No second engine or v1 importer was added.

## Modern NIS source views

`nis::Item` decodes version-1 item headers, nested data layers and child tables
without allocating a tree. It retains UUIDs, flags, reserved words, child
descriptors, trailing bytes and properties in the original source buffer. FOURCC
identities are exposed in logical order while their original encoding remains
accessible. Declared 64-bit extents are checked before narrowing or slicing.

Data layers are validated in a loop with a caller-supplied count limit. Child
extents are validated before iteration; opening a child validates its own layers
and table and can return an error. This deliberately avoids claiming the whole
subtree is valid merely because its outer frame is valid. A traversal caller must
bound total work/depth; the current CLI follows a fixed preset path and rejects
missing or ambiguous required children.

An explicitly unencrypted `EncryptionItem → SubtreeItem` path exposes either
borrowed uncompressed bytes or an owned bounded FastLZ expansion using the same
decoder as NKS. A protected marker returns `AccessRequired` before decompression.
No library key discovery or decryption is implemented. The inner `PresetChunkItem`
exposes its exact chunk payload; checksum/authentication fields remain retained
but unverified. AppSpecific wrappers and other application schemas are not guessed.

Authored fixtures cover opaque/duplicate records, source offsets, child failures,
64-bit length corruption, limits and a 4,097-layer chain without recursive stack
growth. Clear compressed/plain subtrees yield identical bytes, while malformed
and protected sources fail explicitly. The CLI fixture runs the fixed
RepositoryRoot → BNISoundPreset → EncryptionItem → PresetChunkItem profile and
compares its source report to expanded/NKS input. It also rejects duplicate
encryption children. These remain parsing/ownership checks, not library fidelity.

```sh
cargo run --locked -p sampler-native -- inspect-kontakt-nis INPUT.nki
```
