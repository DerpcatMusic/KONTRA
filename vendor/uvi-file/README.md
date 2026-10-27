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
