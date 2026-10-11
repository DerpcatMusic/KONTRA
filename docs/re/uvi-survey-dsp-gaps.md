# Remaining VWinds module gaps (survey item 5)

No DSP law was changed without binary evidence. Ranked follow-ups:

1. **GainMatrix beyond two channels.** Saved matrices route channels 3–6 into
   output 1. Native process `0x141338530` uses the owner's channel count and all
   input/output coefficients (`gain-routing-topology.json`, approved UVI SHA
   `78729e96…5721`). `audio.rs::decode` keeps only the first two channels of
   either a multi-channel file or a mono channel bundle, and sampler-core Frame
   is stereo. Implementing the native matrix requires preserving those source
   channels through decode, streaming, effects and routing. Changing the 2x2
   coefficients cannot restore discarded audio. The existing warning remains.
2. **DigitalEq numbering.** [Vendor parameters](https://lua.uvi.net/_elements.html)
   specify Type1..16 in 0..6 but not the enumeration. The three saved VWinds body
   EQs use enabled types 5/6; disabled scripted bands can still carry other types.
   Cached constructor `0x14128a700`, registration owner `0x141288970`, receiver
   table `0x141e737c0`, and pointer-export `0x14128c590` provide structure, not a
   verified type-to-coefficient law. The compiled-module cache explicitly marks
   the full algorithm unverified. The manual-order warning remains; no enum was
   relabelled from a menu alone.
3. **LFO waveform / live rate / nested source modulation.** The translator's
   admitted waveforms and constant rate folding remain unchanged. Analog square,
   chaos/user waveforms and live frequency modulation require native generator
   and state-update laws, including phase accumulation, smoothing and polarity.
   General modulation API documentation does not supply those laws. No native
   runtime vectors were available and the original executable's relevant code
   is packed; no broad waveform substitution or rate approximation was added.

These are open functionality gaps, not false diagnostics. The module/path message
fix makes them easier to locate without downgrading or hiding them.
