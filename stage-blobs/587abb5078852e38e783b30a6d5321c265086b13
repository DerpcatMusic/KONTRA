# W15: addressed Ladder/Daft cutoff

Physical insert-slot lookup now retains Ladder LP4 (33) and Daft (70/71),
including processors inside dynamic Mix blocks. The shared target translator
uses normalized depth for these native controls. Lowering preserves their
compiled addresses; the worker-reserved filter bank stores normalized deltas
without creating unused SVF coefficient caches. v1 0cb7a8a0:src/engine/filter.rs
adds modulation to saved knobs before parameter conversion; Ladder also
retains its enabled-route flag at zero depth.

Failing-first receipts: w15-native-filter-slots-red.log (physical slot omitted)
and w15-native-cutoff-red.log (lowering rejected the native cutoff route).
Targeted render matches an equivalent static knob change exactly, in 7-frame
fragments, with zero audio-thread allocation/deallocation.

Gate A/B receipt: /mnt/Windows11/DEV_WORKSPACE/kontra-runs/w15-native-cutoff-ab.log.
Conflux's authored LP4/envelope route, exercised at a signed normalized amount of magnitude
0.25, produced residual -7.0148379163 dB relative to dry and normalized
high-frequency energy change -3.0348351074 dB. Analog Strings' authored
Daft/Constant route produced residual -0.1429115902 dB and HF change
+8.0195327377 dB. Both tests retain the saved slot and source/route transforms;
only the amount is exercised. The test selects a key inside the loader's fitted
range; Conflux's short sample ends before the later 0.1-second RMS window.
All PCM/output remains in memory; no decrypted data or WAV was written.

Still open: quiet CPU acceptance, W12 native recount, the existing generic
64-frame source clock versus Kontakt's 32-frame control clock, and Daft's
already diagnosed native audio-kernel approximation. This change executes
retained routes; it does not establish full native dynamic timing parity.
NEXT: LFO:5 execution, then the remaining native knob targets.
