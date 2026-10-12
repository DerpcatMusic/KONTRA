# Vista legacy highpass: 383 candidate

Parent: `415cb383` on `v2/w15-vista-harp`; receipts:
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-vista-383/`.

The violin FFF overlay saves eight enabled type-3 filters, physical slot 5
in groups 16–19 and 48–51, at cutoff/resonance zero. Their full-depth AHDSR
cutoff routes shape legato transitions. Some saved groups have no sample
zones; slot retention must not depend on a populated group. The witness
renders a populated middle-C transition in group 16.

Ported `0cb7a8a0:src/engine/filter.rs::filter_type(3)` into the existing
filter table: one two-pole highpass section, reusing the previous legacy
cutoff/Q law port. Addressed cutoff routes now use the 8.96-octave law for
both legacy types 2 and 3. No render kernel or allocation path was added.

The unit test failed first because physical slot 5 was dropped with two
NotModeled diagnostics. Four targeted legacy contracts now pass. The
real-library witness verifies all eight slots/routes, then renders dry,
static-filter and authored-envelope variants of the same sample in RAM:

| Metric | Result |
|---|---:|
| Static filter residual / dry | −18.194794888 dB |
| Envelope residual / static | −16.026819059 dB |
| Normalized HF change, static / dry | +0.001869372 dB |
| Render allocation guard | PASS |
| Harp lowpass witness | PASS, unchanged metrics |
| Core/KSP/Kontakt test compilation | PASS |

No PCM or decrypted scripts were persisted. Native Legacy HP1 topology,
32-frame native control clock, cutoff bounds and quiet CPU are unverified;
this restores v1's proxy, not certified native parity. W0 owns the batch gate.

Harp source-mode survey: all 20 groups use serialized mode 3, version 0x103,
with timing value −1, free value 1 and timing flag false. V1 also plays mode 3
as a sampler; it supplies no better source kernel to port. The 20 source-mode
diagnostics remain open. W6 retains RandomBipolar/LFO6/source-module work.

NEXT: establish mode-3 native dispatch and active controls before changing its playback.
