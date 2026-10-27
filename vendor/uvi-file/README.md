# uvi-file

Decoders for UVI's file formats, extracted from KONTRA:

- **UFS soundbanks** (`.ufs`, v2/v3): header, directory records, decrypted
  member names and paths, member ranges and protection modes.
- **Member encryption**: the offset-seeded block transform (metadata and
  content keys), and local content-key recovery verified against PNG CRCs.
- **Programs** (`.uvip`): decoding to XML (zip, binary state, base64
  variants) with bounded parsing.

Samples inside banks are ordinary WAV, AIFF, FLAC or CAF, often with an
extension that does not match the codec (Augmented Orchestra stores FLAC in
`.wav` members): sniff the bytes.

`cargo run --release --example census -- *.ufs` decodes every bank given
and prints stage-tagged failures. On 2026-10-27 all 26 installed banks
(VWinds, Augmented Orchestra: 68,299 members, 620 programs) decode fully.

## Member types (census of 26 banks, 2026-10-27)

| Extension | Content |
|---|---|
| (none) | `u32` length + `<Manifest>` XML: the bank manifest (metadata protection) |
| `.uvip` | `<UVI4>` program XML; Augmented Orchestra stores it zipped |
| `.vhfpreset` | `<UVI4>` XML preset |
| `.arp`, `.layer` | JSON (arpeggiator and layer presets) |
| `.lua` | UVIScript source, plain text |
| `.wav`, `.aif`, `.flac`, `.caf` | audio; Augmented Orchestra's `.wav`/`.aif` hold FLAC streams |
| `.asd` | per-sample analysis: a header, then rising `u32` frame positions about one pitch period apart (probably for UVI's IRCAM stretch modes) |
| `.png`, `.jpg`, `.svg`, `.ttf`, `.zip`, `.json`, `.txt`, `.xml` | UI resources and documents |

`examples/census.rs` prints the first bytes of one member per extension; `examples/dump_member.rs` writes members with a given suffix to a directory.
