# BitCrusher callback evidence

This leaf implements the original **BitCrusher / UVI Destructor** callback with physical controls, **Cutoff=0**, and integer **BitSize=1..24**. It is not registered in Program playback. Actual Starter nodes with nonzero Cutoff remain blockers even when Mix is zero.

The [official UVI parameter table](https://lua.uvi.net/_elements.html) lists EffectiveSampleRate 2000..48000 Hz (default 22050), BitSize 1..24 (default 16), Drive and Mix 0..1, Cutoff -1..1, and Bypass. All are modulable. This implementation accepts direct physical controls; it does not claim the hosted smoothing, matrix connections, MIDI or Lua lifecycle.

## Original native boundary

The unchanged official reader executable has SHA256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`. Its retained loaded text has SHA256 `684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`. Probes run the original signal constructor `0x1412d0470`, sample-rate preparation `0x1412d0570`, full effect callback `0x1412d1770`, clone `0x1412d05b0`, and generic bypass wrapper `0x140ecc2d0`.

Only allocation and release hooks provide a caller-owned bounded arena. There are no arithmetic, power, trigonometric, audio-copy, gain or zero-fill substitutions. The authored fixture supplies effect/sample-rate/channel metadata, physical fields and finite audio. Its optional filter has caller-owned already-initialized vector metadata: an 801-entry zero table and a set once flag. Table contents are unused for Cutoff=0; **this is not evidence for the native table initialization** or a full application cold start. The original constructor and preparation execute against that explicitly bounded fixture.

Quantization uses a power of two for integer BitSize. The native negative-value adjustment also applies at exact negative integers: BitSize=4, input -1 produces -15/16 before Drive. Replacing this with ordinary truncation would differ. The rational Drive stage and wet/dry mix preserve the observed float32 operation order.

Sample-hold timing starts from zero at each callback; frame zero therefore always overwrites the held sample. Native held fields persist between calls but have no audible history in this Cutoff=0 boundary. This is why splitting a buffer can change its output. The implementation preserves callback boundaries instead of inventing continuous resampling. Cloning is equivalent only within this scope; the original native clone shares its optional filter holder, which matters with an active filter.

## Functional comparisons

The private authored fixture exercises 32/44.1/48/96 kHz, 1/2/6/12 channels, every integer BitSize 1..24, EffectiveSampleRate 2000/4775.6899/25360.979/48000, Drive 0/.783333/.31089061/1, Mix 1/.1/.5/1, and consecutive partial buffers 1/17/33/257 frames with static control edits.

- **1536 full native callback comparisons, 620928 output samples: Rust and original native PCM are float32-identical.**
- **384 original native clone comparisons:** subsequent callback outputs are identical.
- **32 original generic bypass/resume comparisons:** audio, held samples and optional-filter history freeze through bypass, apart from the generic initialization flag. Resume matches the original clone.
- The Rust leaf also passes clone and dry-bypass checks in every one of the 1536 callback comparisons.
- Three focused functional tests check native negative-integer behavior and callback partitioning, refusal of unmeasured actual controls even with Mix=0, and validation before buffer mutation.

Only these focused authored probes and direct Rust compilations were run. No broad Cargo build or full host playback was used.

## Actual Starter settings and retained gates

The Starter bank was decoded in memory for the two sampled slots known to contain BitCrusher. No vendor XML, audio or scripts were written. The retained safe inventory has:

| Slot / node | EffectiveSampleRate | BitSize | Drive | Cutoff | Mix | Owned connections |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 8 / 101 | 4775.6899 | 5 | .783333 | -.413333 | 0 | 1 |
| 33 / 34 | 25360.979 | 8.7607965 | .31089061 | -.031187475 | 0 | 1 |

Both actual nodes remain unsupported: both have nonzero Cutoff and connected controls, and the second also has fractional BitSize. Mix=0 does not waive those requirements because native filter history can affect subsequent edits.

Attempts to extend the native optional filter exposed unavailable runtime data. Its SIMD sin/cos path reads coefficient arrays around `0x14295bb00` and `0x14295bf80`; its table power path also depends on unavailable runtime data. Running the unchanged instructions against file-backed data produced unstable coefficients and some nonfinite outputs. **Those attempted nonzero-Cutoff results are excluded from fidelity evidence.** No guessed coefficients or replacement math were installed. OnePole coefficient initialization likewise depends on an unavailable runtime constant at `0x14258c220`, so it received no production candidate.

Nonzero Cutoff, fractional BitSize, hosted controls, an active filter's clone state, complete effect-factory initialization and whole-program PCM remain unverified. Program admission is unchanged.

Retained private fixtures and safe summaries are identified by `cutoff0_native.py`, `rust-check.rs`, `cutoff0-native-lifecycle-safe.json`, `rust-callback-comparison-safe.json`, `functional-tests-safe.txt`, and `bank-crusher-safe.json`. Native bytes and decompiler outputs stay private and are not part of this source change.
