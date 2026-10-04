# Initial resource cancellation

The worker's existing stop flag now interrupts initial sample loading between
members and codec packets. CAF and many-channel WAV scalar loops check every
4096 samples. The CLI's existing decode/resource APIs keep their uncancelled
behavior. Successful module loading is followed by another stop check before
Lua initialization. No additional worker pool or thread owner is introduced.

Cancellation has a typed cause. Only that cause becomes normal stopped work;
a real codec error that has already occurred remains a failure even when the
stop flag changes before worker classification. Stopped work cannot publish a
Ready player or initialized controls. A pending mono-assembly rejection can be
superseded by cancellation before the remaining operand decodes finish.

Current-source actual Clarinet comparisons preserve all 158,605,863 PCM scalar
bit patterns, metadata, 196 aliases, 393 progress callbacks and 38,903 packets
when not cancelled. In two controlled resource-stop cases the previous worker
finished all packets and took 2.470/2.416 seconds to join; the candidate stopped
in 6.494/6.329 milliseconds, with zero new packets after the stop request, no
errors or controls, and rejected superseded generation. A separate successful
module-load boundary stops before Lua. Ten focused source checks and the real
codec-error/stop race check pass. These are measured retirement cases, not a
universal maximum cancellation latency or a library loading-time improvement.

Member reads/decryption, metadata/program parsing, image conversion, storage
packing and already executing Lua/renderer preparation are not interrupted
inside those units. Their surrounding checks do not establish hard realtime
cancellation. Private proof and reproducible current-source harnesses remain in
`~/.cache/kontakto-uvi-preload-parallel-private/`; no purchased payload is bundled.
