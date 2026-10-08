# W15: execute zero-weight LFO Multi sources

LFO waveform 5 with five zero weights and an interior pulse width now retains
an explicit bipolar Zero source. Its transformed routes execute normally;
using Constant would be wrong, because Zero's unipolar view is 0.5. The core
returns zero without evaluating an unused waveform or clock. Native nonzero
Multi, live waveform edits and bypassed-source instantiation remain open.

Pinned v1 0cb7a8a0:src/modulation.rs implements a strict version-0x71 sine-only
subset, including zero sine weight. Morphology's version-0x73 records lie
outside that port's admitted source clock. This zero case is own code derived
from the original Multi kernel's weighted-sum law, not copied pseudocode.
The native PE hash is 0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8;
original entry 0x140b07290 returns zero at nine phases and three pulse widths
(27 checks), without helper substitutions. Receipt:
/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-native/multi-waveforms.json.
The isolated waveform check does not certify the saved parameter setter or
host source clock. Zero output is independent of delay/fade/phase.

Failing-first: w15-zero-multi-red.log rejects the authored source. Targeted
source/render tests pass; fragmented render, delay/fade, note release and
voice reuse make zero audio-thread allocation/deallocation calls. Morphology
A/B through an exercised bipolar volume route is exactly half the dry output,
-6.020599913279624 dB, with all PCM kept in memory and no WAVs written.
Receipt: /mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-zero-multi-ab.log.

Quiet CPU acceptance and W12's full native-source recount remain pending.
NEXT: nonzero Multi waveform/clock and remaining native knob targets.
