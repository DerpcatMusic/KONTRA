# Falcon meter requests: exact identity, explicit unavailable taps

The AudioMeter constructor previously routed `isVertical` into a stereo flag,
`busId` into a channel index and `useOutput` into orientation. It now preserves
those three independent arguments according to the official
[AudioMeter contract](https://lua.uvi.net/class_audio_meter.html).

The shared UI IR retains a typed `UviMeterRequest`: promoted element ordinal,
actual XML NodeId, Program/Layer/Keygroup/Oscillator scope, authored bus ID,
input/output selection and mono-display request. The two identity numbers are
different. XML identity comes from the same retained promoted source map as
script parameter ownership; it is not inferred from widget order or DSP packing.
Keygroup/oscillator scope identity must agree with that XML node. Invalid targets,
fractional/negative/overflowing buses and malformed policies remain diagnosed.

The renderer previously substituted Program output for an unavailable element
tap. Falcon LevelMeter now accepts only its explicit provider readback; it does
not fall back to the unrelated legacy output service. Kontakt's existing output
meter path is retained. An absent or invalid Falcon request paints no synthetic
activity. This is an ownership correction, not delivery of native meter support.

Two authored UVI tests verify four element scopes, both orientations, distinct
bus/input/output policies and malformed/fabricated requests. A production CPU
paint fixture checks that an unavailable input/element request never calls or
paints a supplied unrelated Program-output provider, while explicit resolved
provider values and the existing Kontakt output path remain effective.
The focused request suite passed 2/2 and the root meter/painter regression 11/11
on 2026-10-14; existing UVI UI, asset and property suites also passed.

The [raw bus-tap API](BUS_METER_TAPS.md) is a separate core boundary. Exact UVI
XML-element-to-physical-bus mapping and its production bridge still need proof;
missing layer/element buses must not be mapped to Program. `displayAsMono` and
0 dB colour diagnostics remain until there are genuine consumers. The reference
describes mono as a sum but does not establish normalization or a dB-to-pixel
display law. Adding scalar peaks, averaging channels, or inventing meter activity
would not supply that missing evidence.
