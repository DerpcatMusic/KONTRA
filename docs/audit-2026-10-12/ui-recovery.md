# Recovery of the W13 modulation UI work

Donor: read-only inspection of
`/home/derpcat/.t3/worktrees/KONTAKTO/fix-w13-product-parity`, branch
`v2/w13-product-parity`, committed tip
`cb728a1cc804f3e75d60c8435728ab4f01ef16b5`. The recovered behavior was an
uncommitted change in its `src/ui/chain.rs`, `src/ui/editor.rs`, and
`src/ui/v2_tests.rs`. Its local handoff, `docs/audit-2026-10-08/w13-live-modulation-parity.md`,
says focused checks passed there but root no-run and push were pending after
a W9 quiet-window request. That is the donor's stated reason, rather than
proof of a still-active blocker. Its receipt paths no longer exist here.
The donor worktree is preserved.

The current integration branch had already recovered the donor tip's
group-insert inventory. Two dirty follow-ups remained missing: the
modulation list omitted primary amplitude envelopes, and key/velocity
source bars never showed a live value. Source reference `0cb7a8a0:src/ui/chain.rs`
provides v1's envelope inventory and normalized key/velocity display.

## Recovered behavior and source evidence

- List each distinct `Zone.amplitude` of the selected group as
  Envelope → Volume at unity depth. Preserve the existing explicit-route
  inventory and ignore invalid route/modulator references.
  `crates/sampler-ir/src/lib.rs` documents this field as the voice amplitude
  envelope. In `crates/sampler-kontakt/src/library.rs`, the first admitted volume
  envelope takes the `envelope` path instead of adding an explicit route,
  and zone construction assigns `amplitude: group.envelope`.
- Pass the part slot from the Sound editor to the modulation list. Read
  velocity/127 and note/127 from the last matching active voice tap in that
  part's existing bounded 16-tap array. Read them only while the slot owns
  `editor_watch`; another part's or group's taps do not light these bars.
  Existing CC1, pitch-bend and Constant source readouts are preserved.
- Keep Graphite Rack controls, typography, spacing, neutral fills, and
  existing canvas bars. No authored Kontakt/Falcon rendering or library
  assets are changed. The Impeccable context and craft-floor instructions
  were read against the existing native UI and persisted Sound surface.

`src/plugin.rs` already publishes each part's editor taps only when it is
the watched slot and its generation matches the runtime. This recovery
uses that publication without changing the audio path, locks, or DSP laws.
The normalization also matches `ModulationSource::Key`'s documented IR
meaning and v1's live-source function.

## Validation

The strengthened donor regressions fail against the previous behavior:
one observes unchanged rendered velocity pixels; one finds zero Envelope
rows. They also cover the watched second part, another group, an old
watch owner's cached taps, shared primary envelopes, and malformed IR
references. Red receipt: `/tmp/kontra-ui-recovery-red.log`.

After the port,
`/home/derpcat/.cache/kontakto-heavy cargo test --profile ci --lib v1_modulation_`
passes **2/2**. Green receipt: `/tmp/kontra-ui-recovery-green.log`.
The native CPU-rendered state comparisons verify the live bars; scene-text
checks verify the envelope row. The existing Graphite Rack screenshot was
inspected for the incumbent visual language. No new presented-host screenshot
or full-window rendering is claimed by these focused checks. The Impeccable
mechanical detector returns `[]` for the touched chain and editor surfaces;
`git diff --check` passes. The coordinator owns the final full root suite.

## Limits

This is a bounded source-value readout, not a native Kontakt/Falcon audio
parity measurement. It shows the last matching published voice among at
most sixteen taps. It does not add live envelope/LFO source values or fix
shaped/inverted/scaled route visualization. The pre-existing native host
and whole-library scan work remains separately measured by the coordinator.
