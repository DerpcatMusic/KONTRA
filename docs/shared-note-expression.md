# Shared note expression

The shared engine keeps note identity, expression identity, physical source state and per-attack processing ownership separate. Two notes on the same key can use the same immutable sample storage while retaining independent cursors, filter histories and expression values. Script-generated children can follow their parent's pitch, pressure, timbre and bend while retaining their own gain, pan and release policy.

An optional `SignalParameter.expression` maps the retained note's full-resolution pressure or timbre to a factor on its ordinary base parameter. Gain supports a nonnegative linear factor; OnePole cutoff supports an octave factor. Global control edits, ramps, snapshots and recalls continue to own the base. They do not replace either note's expression value. The compiled mapping and every combination of its endpoint and base-control domain are validated before publication, including the actual sample-rate cutoff limit.

Shared stages retain one filter history per processing owner and channel. Expression changes do not retrigger sources, clone plans, reset histories or mutate another note's filter. Missing expression bindings preserve the existing signal. Explicit signal nodes execute at their authored graph position, before a nonempty legacy scope chain; this is not an automatic post-chain effect injector.

Existing stereo source Tone remains after the authored voice chain on the main outlet. Its per-note state is distinct from shared bus effects and existing earlier send/tap positions. Public loader default-Tone tests and the product's adaptive Kontakt mapping are different recipes and must be evaluated as such.

The MIDI adapter supports explicitly configured lower or upper MPE zones. Host-note admission retains initial tuning separately from member/manager bends and part transpose. Native host expressions and terminal events use captured note identities, including pending aligned notes, rather than a Part's later input-port or channel setting. Automatic MCM/RPN6 zone negotiation remains unsupported; this is not a claim of complete automatic MPE receiver certification.

The resident channel graph and these optional enhancement mappings are shared engine capabilities. They do not establish native sampler DSP parity, activate every importer recipe, make a shared bus per-note, or establish actual VWinds admission. Streamed wide-source resource budgets and unresolved native layout/scope laws remain separate work.
