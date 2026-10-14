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

The [raw bus-tap API](BUS_METER_TAPS.md) is a separate core boundary.
`displayAsMono` and 0 dB colour diagnostics remain until there are genuine
consumers. The reference
describes mono as a sum but does not establish normalization or a dB-to-pixel
display law. Adding scalar peaks, averaging channels, or inventing meter activity
would not supply that missing evidence.

## Conservative raw production bridge

`sampler_uvi::meters` now carries ancillary XML ownership outside serialized IR
caches. Only a unique Program/Layer default-bus (bus ID 0) mapping is admitted.
It joins the source insert node to its actual chain, verifies that the chain has
bus scope and is that bus's owner, and uses the nearest source synthesis owner.
It does not infer an address from widget order, promoted ordinal, layer number
or parent output. Auxiliary buses cannot alias Program. Collapsed/omitted scopes,
per-voice keygroup/oscillator chains, ambiguous owners and other native bus IDs
remain explicitly unavailable.

Attached scripts with initial meter requests carry this map into `ScriptUi` and
the prepared Part. Only initially requested, resolved input buses are enabled
off audio. Later input requests that were not enabled remain `InputNotEnabled`;
when no ownership map was prepared they remain `OwnershipUnavailable`. A plan
replacement invalidates the map rather than reinterpreting its old indices.

The plugin publishes raw stereo observations after render/end-block and before
the existing mixer output drain. The previous 100 ms widget poll runs before
render and cannot supply these drained physical peaks. Input observations drain
independently after publication. The callback uses bounded existing cells and a
nonblocking mutex attempt; zero registered interest skips that path. At most 512
requests are retained, with an explicit interest-limit result. UI registration
scans the authored graph once and then works on its meter requests. Both stereo
channels publish in one atomic word. Generation checks inside the registry lock
and per-cell owner epochs prevent a replacement UI from inheriting old peaks.

`InputState.uvi_meters` exposes typed raw availability, physical ownership and
stereo peaks separately from the legacy scalar meter map. No mono sum, channel
normalization, ballistics or native dB/pixel law is fabricated. Raw observations
do not drive the current scalar painter or suppress the existing unsupported
property diagnostics. This is a source/production readback boundary, not proof
that native AudioMeter pixels now match or that the installed binary was updated.

Authored fixtures exercise real translation/attachment without an audio host,
exact bus/chain ownership, auxiliary isolation, collapsed/per-voice/ambiguous
rejection, independent input/output levels, publication before drain, plan and
UI generation replacement, invalid observations, bounded/pruned interest and a
busy UI mutex. Callback publication/drain checks make zero allocation/free calls.
The CPU painter fixture also supplies nonzero raw stereo data and verifies that
it cannot paint an unrelated or invented native scalar meter; the Kontakt output
provider remains separate. No real-bank native meter law or pixel comparison is
claimed by these fixtures.

Final focused validation on 2026-10-15 passed three raw publication/runtime
fixtures, the CPU paint guard, and all three UVI ownership/attachment fixtures
(five tests matched the ownership filter, including two existing parameter
tests). The generation-replacement and busy-registry assertions are included in
those counts. These checks do not establish native meter display parity.
