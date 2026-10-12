# W15: Filter 90 v1 port

Ported v1 `0cb7a8a0:src/engine/filter/models.rs::formant`: five vowel
formant triplets, Talk interpolation, Size transpose, Sharpness Q, three
15 dB peak sections and quarter-level trim. Voice and bus inserts use the
existing IR biquad consumer. The translator retains an explicit UnknownLaw
receipt: this is v1's audible proxy, not certified Kontakt coefficient parity.

Failing-first: `v1_formant_slot_is_an_executable_processor_in_both_scopes`
failed before the port. Actual Analog Strings gate-sample A/B changed the
level-normalized derivative-energy spectrum metric by +0.644500491 dB;
dry RMS [-23.5093393, -14.1032004], wet [-35.2949577, -25.9617654] dB.
PCM/output stayed in memory. Numeric receipt: `w15-offline-ab.log` under
`/mnt/Windows11/DEV_WORKSPACE/kontra-runs/`.

Dynamic Talk/Size/Sharpness routes, native coefficient calibration and quiet
CPU comparison remain open; these are required before the DSP gate can pass.
NEXT: LoFi port, processor-parameter addressing and native filter kernels.
