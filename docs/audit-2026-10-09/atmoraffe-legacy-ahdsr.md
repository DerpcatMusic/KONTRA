# Older AHDSR records: Atmoraffe

The user-supplied free Atmoraffe library reproduces a native-reader failure in
published Linux KONTRA 0.3.344 (`7cd326ee5b67cb78f22fe235bd4b26aaebb0c291`).
Both main instruments fail before sample preparation with AHDSR revision
`expected 17, got 16`. These are serialization revisions 0x11 and 0x10,
not modulation-name counts or evidence of an added delay stage.

The bounded older record is 44 bytes: the unstructured flag, u16 revision,
six f32 timing/curve/level fields, a mode byte, and 16 opaque trailing bytes.
The existing 0x11 codec has the same known prefix and at least 52 opaque
trailing bytes. The reader admits each supported layout explicitly; the writer
preserves its revision and opaque metadata. No timing field is dropped.
Malformed/truncated records and unknown revisions remain rejected.

Authored tests cover unchanged round trips, edited timing, mode/tail retention,
version/layout mismatch, truncation, nonfinite values and unknown revisions.
These tests and private production-loader/PCM/runtime checks are **pending**;
this patch is not READY until their actual results are recorded.

The two main instruments and all 610 supplied WAVs will be tested privately.
A third NKI under Samples is an older XML NKS document and fails the binary
chunk reader separately; the envelope change does not add XML import.

Official CLAP, VST3 and standalone binaries were installed only under the
user's separate explicit installation request, with original files/settings
backed up. Installed CLAP SHA256:
`264017ceb04bfdea5e133dd9ed427970134c54c245d2eb48b8a4c06919b9539e`.
The shipping CLAP baseline fails both main instruments on the same envelope
revision check. No sample bodies, script bodies, keys or PCM are committed.
Private installation and baseline receipts:
`~/.cache/kontra-atmoraffe-test-20261009/{installation,playback}.json`.

No CPU timing or DAW visual verification is claimed. The independent modern
FileContainer work is parked outside this change and is not included in the
validation candidate.
