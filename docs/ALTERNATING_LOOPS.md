# Alternating sample loops

The importer preserves the native loop direction flag. Zero-crossfade alternating loops use the existing shared playback map: a virtual frame sequence reflects at the first and last sample frames of the loop, visiting each endpoint once per turn. The resident reader and streamer use identical ascending or descending runs, including the interpolation stencil at fractional positions. No audio-thread allocation or separate resampler is introduced.

Until-end paths keep reflecting during release. Until-release paths resume the normal playback direction from the current sample frame; the existing voice retains its three-frame interpolation guard when releasing. One-frame loops hold their frame. Forward loops keep their existing mapping.

The [Kontakt manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view) documents the forward/backward ping-pong direction and the until-end/until-release distinction. That manual does not specify endpoint duplication or fractional interpolation. The continuous reflection convention here is covered by independent authored frame and PCM references; exact Kontakt interpolation parity has not been measured. The existing binary mode-2 interpretation as until-release remains unverified against an actual mode-2 preset.

Crossfaded alternating loops retain their metadata and an explicit warning; playback uses the existing forward-crossfade path until the alternating crossfade law is established. Counted or tuned loops retain their existing unsupported warning. Native export writes the direction flag, and the compact import cache distinguishes both alternating modes. Import and cache source changes invalidate earlier cached imports through `KONTRA_IMPORT_HASH`.

The actual Una Corda Pure, Felt, and Cotton metadata each contains 31 active loops: 28 alternating and three forward, all until-end, infinite, untuned, and zero-crossfade. The ignored test `actual_una_corda_alternating_loops_are_preserved` checks this metadata without committing library content; set `KONTRA_UNA_CORDA_INSTRUMENTS` to the local Instruments directory to run it. The authored playback test compares an alternating loop to independently unrolled PCM at source steps 0.5, 1, 3.25, and 32, including release. The existing streamed/RAM comparison also exercises alternating loops in both directions.

Combined build `80dbb6f` passed the map, cache, fractional/high-step PCM, streamed/RAM, and actual three-preset metadata checks. A separate probe linked against that exact production rlib selected one actual alternating sample per Una Corda preset: Pure zone 3725, Felt/Cotton zone 3726. Each sample has 475,512 frames at 48 kHz and a loop spanning 33,761..462,673. Rendering 3,036,139 stereo output frames per preset across three complete reflection periods matched independently unrolled real PCM bit for bit, with maximum absolute error zero and nonzero audio. Release output also matched bit for bit. This check preserves native zone gain, pan, tuning, start, and loop metadata while removing scripts and group effects/modulation to isolate the sample path; it is not a complete instrument or Kontakt reference comparison. Reproducible probe source, aggregate results, and hashes are retained in the ignored `artifacts/combined-80dbb6f` directory.

## v2 selection work (W7)

The v2 IR now retains all eight physical loop slots, including holes, count,
tuning, alternating direction and crossfade metadata. Lowering consumes finite
counts through the existing cursor exit law. Serial loops use the same address
and interpolation law in resident playback and streaming; the ordinary single,
untuned slot uses the established fast cursor. Explicit release and reset tests
cover the engine contract without claiming a measured Kontakt law.

Native parity gates remain **pending native vectors** for mixed criteria
precedence, loop count exhaustion, tuning onset, alternating crossfade, and
multiple-loop ordering. Overlapping loop ranges currently reject preparation.
Alternating crossfade retains its metadata and an `UnknownLaw` finding while
using reflection; the exact transition must be replaced with the measured law.
The fresh static seam verifies serialized criteria joins as AND=0, AND_NOT=1,
OR=2, but does not yet establish mixed-operator precedence.
