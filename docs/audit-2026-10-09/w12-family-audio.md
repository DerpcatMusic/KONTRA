# W12 opt-in host family observations

`KONTRA_FAMILY_AUDIO=1` adds bounded main-stereo capture to the existing live CLAP host. PCM stays in RAM (at most30 seconds at48kHz, about11MiB). After rendering finishes, it reports whole-instrument and note-window onset/length at−80dB relative to each window's peak, peak/RMS, and32 normalized log-frequency Goertzel powers. Window boundaries are scheduled note-ons; overlapping notes, FX and previous tails remain part of that window. These are output fingerprints, not voice/sample IDs or isolated dry samples.

`observe` retains only numeric `note_audio` records and marks the observation separately. `measured_status` returns UNKNOWN for these runs, so they cannot supply CPU/load acceptance. Keep the environment flag absent for W9's base timing protocol, and use normal sample-exact host events (`cpu_audit=False`) for RR schedules. W9's original4-second raw audit tuples use `cpu_audit=True` and retain its block quantization; their timing outputs remain excluded from acceptance. Simultaneous note-ons share the same mixed-output window and cannot identify an individual voice. No second host wrapper or frozen-v1 rebuild is added.

`family_audio.compare(v1,v2)` rejects mismatched schedules, native Selection readback failures, underruns, nonfinite or incomplete receipts. It reports ordered onset/length deltas and spectral cosine separately from the repeated-key mean-spectrum distribution. Reordered RR hits can differ individually while their distribution agrees. Silence has no spectral similarity. The numeric comparison uses frozen v1 as reference and makes no native-host claim. Family identity remains UNKNOWN; no raw-PCM correlation or null reducer is implemented.

Validation: Python authored checks cover identity, silence, nonfinite data, complete schedules and RR reorderings. Host `--self-check` adds analytic onset/length, opposite-phase stereo energy and spectrum normalization checks. No real-library fingerprint comparison or native family certification is claimed by this tooling commit.

NEXT: run the analytic C++ checks and the first matched frozen-v1/v2 observations, then adjudicate the largest numeric lead.
