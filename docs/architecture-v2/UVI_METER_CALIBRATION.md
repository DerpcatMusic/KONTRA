# Native AudioMeter calibration boundary

The current production bridge supplies exact-owned raw stereo observations.
It does not establish native meter detection, mono, geometry or animation laws.

The official [AudioMeter reference](https://lua.uvi.net/class_audio_meter.html)
specifies element-bus attachment, input/output selection, bus ID and orientation.
Mono is described as one summed multichannel display. It does not specify whether
sample summation precedes detection, normalization, clipping or temporal pooling.
The documented colour ranges are `overColour >0 dB`, `0dBColour [-3,0]`,
`3dBColour [-6,-3]`, `6dBColour [-10,-6]`, and `10dBColour [-inf,-10]`.
Shared endpoints leave exact boundary selection unresolved. The reference also
provides strip-image/frame count and scale-display options, without an exact
amplitude-to-frame/pixel law, peak hold, refresh cadence or decay specification.
The official [KSP porting guide](https://lua.uvi.net/_ksp_porting.html) describes
attachment to a tree node's bus, including inserts; the conservative Kontra
Program/Layer bus-0 bridge is therefore a subset rather than complete ownership.

## Existing local binary candidates

A bounded read-only query of the existing `Workstation409` Ghidra project found
registration data associated with source executable SHA-256
`78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`.
The analyzed memory and matching executable's disk `.text` differ. These are
analysis-memory candidates; do not execute disk bytes at these addresses or
claim isolated-byte parity without resolving that provenance first.

| Registration evidence | Candidate locations |
| --- | --- |
| AudioMeter name | string `0x141e42fb8`, reference `0x14105139a` |
| displayAsMono | string `0x141e42fd8`, reference `0x141051475`, registered function pair `0x1410bf0a0` / `0x1410db970` |
| 0dBColour | string `0x141e43048`, reference `0x141051506`, pair `0x1410bc660` / `0x1410db200` |
| 3dBColour | reference `0x141051523`, pair `0x1410bc680` / `0x1410db220` |
| Other colour bands | references `0x141051540`, `0x14105155d`; inspect the adjacent registrations before assigning a band |
| setStripImage | string `0x141e43098`, reference `0x141051573`, registered function candidate `0x1410de3d0` |
| Shared registration body | PE unwind range `0x14104e3b0..0x1410525cd` |

R8/R9 registration pairs are candidates for property accessors, not yet verified
getter/setter signatures. Registration proves API dispatch targets, not a detector
or painter. Follow those accessors to object fields and the associated paint /
timer / audio-provider consumers before assigning semantics. No original runtime
or original host was launched for this query; no native meter law was measured.

## Bounded original-runtime fixture

Use an owned minimal program and generated PCM, with no vendor library or artwork.
Pin the original application/plugin image, sample rate, block size, GUI scale,
display refresh, window size, all envelope/gain/pan settings and parameter state.
Capture actual output PCM as well as timestamped native meter regions. Associate
audio frame counts with monotonic capture times; a screenshot alone cannot
determine refresh or peak-window boundaries.

1. **Identity and routing.** Two independent layers with distinct steady levels;
   Program and each Layer input/output meters; one known bus gain and one muted
   branch. Check a nested insert target separately. An unsupported/collapsed scope
   must stay unavailable in Kontra, never inherit a master tap.
2. **Mono detection order.** At a low unclipped amplitude use left-only,
   right-only, identical L/R, opposite-phase L/R and quarter-cycle phase-shifted
   pairs. Add separated positive impulses in L/R within one audio block. These
   distinguish signed sample summation, normalization and peak-domain operations;
   no sum or average of channel peaks is assumed.
3. **Stereo layout.** Asymmetric L/R levels, then swap channels and orientation.
   Measure channel order, gap, dimensions and background at two widget sizes.
4. **dB geometry and colours.** Give every band a distinct solid test colour.
   Sweep steady amplitudes through silence and below/at/above each documented
   threshold, plus an overload case. Measure filled extents and boundary colours
   at two sizes with scale on/off. Derive, rather than assume, floor, rounding,
   clamping, strip-frame selection and scaling.
5. **Time response.** Step and gated signals followed by silence; 1/64/256-frame
   impulses at several offsets relative to audio and GUI frames. Compare rise,
   decay, hold and refresh at 48 kHz blocks 64/256, then repeat only discriminating
   cases at 44.1 kHz. Separate sample-peak detection from display ballistics.

Bound every case, retain numeric pixel runs/colours/timestamps and PCM hashes,
and report missing observations explicitly. First establish steady-state and
timing capture repeatability. A minimal harness loading the original runtime is
a calibration comparator; the standalone Kontra inspector remains independent
of the plugin binary and DAW. The fixture above is a proposal, not an executed
reference-host or native-pixel parity result.
