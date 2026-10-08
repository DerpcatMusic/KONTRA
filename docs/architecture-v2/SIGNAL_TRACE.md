# Shared signal trace

Enable `KONTRA_SIGNAL_TRACE=1` and set `KONTRA_REPORT_DIR` to a fresh run directory before loading an instrument. Kontakt, UVI and the plugin use the same core recorder. No release install is required. A caller can instead opt in with `Prepared::with_signal_trace(record_capacity)` and drain `Runtime::signal_trace_reader()` on a control thread.

The report worker writes `signal-trace.json` and `signal-trace.svg` under the run directory. Additional runtimes have numeric subdirectories. The plugin Logs tab links both files; the diagnostic report includes their paths, record/drop counts, completion and I/O status. Native CLI renders flush the collector before exiting.

Schema 1 contains:

- `graph`: sample rate, fixed node table, parent/child edges and a topological node order. Labels are public processor kinds; identities are numeric. Nodes cover raw resampled sources, pre-amplifier FX, envelope/velocity gain, post-amplifier FX, zone output, bus sums, ordered bus FX, faders, sends and the runtime master. Plugin adapters also record the part fader, auxiliary send level, rack bus fader, per-frame master gain and physical host output.
- `records`: absolute frame offset, block length, input/output stereo peak, RMS and DC, applied multiplier, algorithmic latency, enable/bypass state, live parameter values and native readbacks when the parameter has an address/law. Amplifier rows use the playing voice’s envelope times and curvatures, including script changes; encoded time readbacks reflect whole-frame quantization.
- `contribution: true`: a single voice at the zone-to-layer boundary. Its numeric identity includes zone, group, layer bus, sample, family/generation, RR sequence/take, pitch ratio, source/start frame, note velocity, CC1/7/11, region/velocity/crossfade gains, script/note gains, envelope level and actual amplifier control gain. Other records are coherent per-node sums. Do not add contribution rows to those sums a second time.
- `dropped`: bounded-buffer overflow count. A nonzero count invalidates a complete time-series comparison. `complete` becomes true after the render writer is retired and drained.

Nodes that did not execute have no row in that block. RMS/DC of a sum are measured after summing the signals, preserving phase cancellation. The graph’s `gain_measurement` distinguishes an applied scalar multiplier, mix coefficients and an effective energy ratio (processors without a single scalar multiplier). A scalar multiplier and measured RMS delta are separate: a filter or stereo matrix changes signal energy without having a scalar volume control. Creative delay time is a parameter; latency denotes the pure delay path when no dry path is present. Convolution's implemented algorithm adds zero scheduling latency.

Preparation allocates the graph, per-node planar accumulator and bounded SPSC queue. The callback writes fixed slots, sums metrics and publishes plain records; it does not lock, allocate, serialize or write files. The producer is wrapped only to satisfy the existing generation's `Sync` bound; callback access uses `Mutex::get_mut`, never `lock`. Tracing selects a separate scalar render specialization. Trace-off keeps the existing batch/parallel specialization and allocates no trace storage. Performance scores should be collected with tracing off.

The trace contains no PCM, sample/library paths or names, and no script source. A native reference WAV supplies final-output comparison only; absent native internal taps, a native per-stage value must be explicitly identified as inferred rather than measured. The pinned v1 engine has meters and tail detection but no equivalent exported per-node trace to port.

## Analog C4 evidence

The bounded diagnostic uses C4 velocity 100 with GRID CC1=64, CC7/11=127. The first loss-free capture has 164,404 records and zero drops. Two voices sound: zone 28307/group 88 on layer bus 5, and zone 46025/group 139 on layer bus 6. Both have constant velocity response and unit static crossfade weight. Their zone outputs are −26.304 and −31.835 dBFS; after the layer racks/faders, outputs are −26.448 and −38.235 dBFS. Their coherent insert sum is −26.360 dBFS, only +0.088 dB over the louder layer.

The insert EQ changes that sum by +0.059 dB. The compressor kernel changes it by 0.000 dB in this window. Global insert slot 1's output value 560434 applies +8.994 dB, producing −17.307 dBFS; the return changes final output to −17.136 dBFS. The output trim matches the separately measured native on/off delta (~+8.44 dB); changing that law would mask the upstream mismatch.

At CC1 0/64/127 with 0.5 s settling, group-volume readbacks for the two active groups remain 680578/629888. Their decoded amplitude routes use CC7, not CC1. No CC1 volume/output-gain writes were observed. Unsupported INTMOD/LFO writes remain diagnosed. This saved state does not support the hypothesis of two equal full-level layers replacing a CC1 crossfade. The native session uses CC1=0 while GRID uses 64, and RR choice is not deterministic; compare distributions rather than matching a sample identity.

The root of the upstream excess remains under investigation. The first Analog report predates the plugin mixer extension. Native internal-stage taps remain unavailable.

Plugin rack/master rows observe the coherent shared rack signal, including other parts on that rack. Candidate routing edges are qualified by each row’s numeric `routed_to` and `external_port` identity. The `host_master` row measures a stereo rack after global gain. `host_output` then observes the actual host channels after mono conversion and summing every rack mapped onto that port; `output_channels` identifies mono versus stereo. The right metric of a mono port is zero, preserving physical channel identity. Auxiliary send level has its own node. These post-instrument observations can include other parts on the same physical port.
