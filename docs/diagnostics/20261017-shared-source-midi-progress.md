# Shared source and MIDI progress — 2026-10-17

This is a tested implementation milestone on the universal-channel-sources branch, not a full sampler-parity or installation acceptance.

## Resulting behavior

Normal guarded WAV/AIFF/NCW multichannel sources can retain ordered physical channel packs through the normal full/streamed source owners. One logical cursor, interpolation boundary, amplitude owner and synchronized all-pack readiness are retained. Preparation and decoder workspaces, start heads, page pins, group residency and retirement account for the physical dependencies. The checked encoded PCM route uses the existing normal decoder and refuses unsupported geometry before publication. Arbitrary custom readers, FLAC and multipart wide admission are not certified by this milestone.

Raw MIDI2 note attributes, per-note bend/range/cohorts, pressure, silent zero-velocity gates and physical note-off FIFO preserve actual protocol/port/group/channel identities. Raw per-note events cannot borrow unrelated native host notes. Explicit native host projection remains a separate policy. Ordinary MIDI2 CC74 now reaches the captured channel-controller pipeline rather than incorrectly acting as selected-MPE timbre.

Physical native host key ownership survives a cancelled or retired Core voice. Exact later key-up, choke, All Notes Off and transport stop retain distinct callback/physical acknowledgment lifetimes without inventing VST3 CLAP NOTE_END or replaying Lua release after synthetic stop. Source-backed and raw physical debts and pending callback receipts have separate checked aggregate limits. Existing allocation/free guards exercise these paths.

CC121 resets the represented selective controller set, not bank/volume/pan/per-note sensitivity or unrelated addresses. Captured delayed controller work retains its epoch across forks; a later fresh release callback can observe the new epoch. Host transport events apply at their actual process offsets without refilling callback budgets. Fixed-size CLAP MIDI storage now accepts semantic short messages without interpreting padding as MIDI data.

KSP binding preserves existing authored initial controller levels. The former empty-bind replacement erased Kontakt's saved volume seed and silenced an otherwise correctly pitched generated child; the normal importer comparison now passes its original PCM oracle.

## Actual evidence and remaining failures

Private immutable receipts are under `PluginArtifacts/kontra-universal-channel-sources-20261017-r19/` on the development drive. Different source-cohort identities remain separate:

- `milestone-quality28`: affected full libraries have **2,060 passed, 1 failed, 85 ignored**. Twenty nested subprocess summaries are excluded from these top-level totals. The one failure is `plugin::exact_packet_tests::fixed_clap_storage_short_messages_reach_the_real_receiver_without_double_callbacks`: an actual channel-pressure packet still does not enter the captured KSP129 callback. It remains an active regression, not an ignored or waived test.
- That same frozen whole source passed all-target Clippy across all fourteen first-party package roots with shots, and the corrected pinned same-policy Doctor producer completed with zero producer errors and zero error diagnostics. The report retains **12,005 warnings and 85 informational findings**.
- `native-profile26` and `native-profile27`: MIDI Ingress17, MPE32 and per-note19 tests passed; the original cancelled/native global-stop regression passed both modes. The physical-owner selection also passed root14/Core7/Kontakt25/UVI11 cases, including synthetic-stop then actual acknowledgment, exact source/velocity, alignment and allocation checks. Host expression and accepted per-note future-state regressions passed.
- `protocol-fixtures24`: the migrated physical-channel KSP controller tests passed18 and the corresponding Core tests passed12. Authored/global banks remain distinct from raw physical input.
- Earlier `reset-stop-ksp21`: eleven normal-importer expression/PCM tests and seventeen source-attachment plus forty-seven channel-topology cases passed on their separately recorded source identity. These totals are not substituted for current full-shipping evidence.

The canonical Core receiver migration is still required for correct captured virtual bend/pressure/reset consume/transform/wait behavior. Independent official-contract review also found that UVI defined event callbacks own forwarding, unlike KSP's default-forward model; the current UVI unconditional original-controller forwarding still requires correction. These are real open semantics, not solved by successful packet conversion.

FLAC remains a separate unaccepted candidate: independent review found CRC-valid malformed predictor/wasted-bit arithmetic that can panic in the pinned decoder. A source patch, valid-predictor comparisons, malformed-owner refusal and exact MPL source packaging must pass before enabling that route. This milestone does not silently enable it.

Actual VWinds Clarinet A's six-channel, three-player keygroup, retained insert/shared-scope graph and native source-profile admission remain unfinished. Static native byte/interpolation/queue evidence is not equivalent to an activated reference playback comparison. No all-UVI/Kontakt parity, unrestricted loading, cold-time or zero-CPU claim follows. This work has not replaced the installed0.3.503 binary or passed the final shipping/native-editor/signing/install/rollback gates.
