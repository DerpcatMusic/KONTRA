# W13: primary envelopes and live key/velocity source bars

One row from the 84-axis inventory: **Source/group modulation assignment names, depth/invert/lag and live wheel/key/velocity source**. The broad Present label missed primary amplitude envelopes and the key/velocity live bars: v2 inventoried only explicit routes and handled only CC1, pitch bend and Constant at paint time.

Port from v1 0cb7a8a0:src/ui/chain.rs, `live` and `depth`: read key/127 and velocity/127 from the watched group's last published voice tap. Adapt the owner check and tap lookup to v2's per-part bounded atomics. The existing wheel/constant cases remain in the same paint callback; no audio publication changes are needed.

Port the envelope assignment inventory from the same v1 file's `modulation`. Include each distinct selected-group `Zone.amplitude` as a 100% Volume assignment. Kontakt's translator puts its primary, unshaped, non-inverted unity-depth AHDSR here instead of in `Zone.routes` (`crates/sampler-kontakt/src/library.rs:875`, `:952`). Reuse `Route::new` for this read-only row and retain invalid-reference checks. Zones sharing the same amplitude modulator produce one row; other groups remain excluded.

Failing-first `ui::v2_tests::v1_modulation_bars_follow_live_key_and_velocity_of_the_watched_part_group` failed because changing velocity left the rendered image unchanged. Its synthetic CPU-rendered fixture checks velocity and key independently, excludes taps from another part/group, and hides a former probe owner's cached taps.

Failing-first `ui::v2_tests::v1_modulation_inventory_includes_primary_amplitude_envelope_once` failed with zero Envelope rows. Its scene-text fixture checks one Envelope → Volume +100% row for two zones sharing their primary envelope, excludes another group's envelope, and rejects the empty-list message. Both targeted regressions now pass (2/2) through `kontakto-heavy`, `ci` profile. Receipts: `~/.cache/kontakto-w13/product-parity/{modulation-red,envelope-list-red,modulation-combined-green}.log`.

Root no-run and push are pending: W9's quiet request arrived after the targeted job finished. No W13 heavy job or waiter remains.

This restores source readouts over the existing 16-voice probe, matching v1's bounded capacity. It does not establish shaped/inverted route visualization, DSP modulation parity, or presented host pixels. Frozen 0.3.326 does not include this follow-up.

Suggested release note: “The Sound editor now lists primary envelopes and follows played key and velocity in modulation bars.”

NEXT: root no-run and READY after the quiet request clears; source-only remaining unowned settings parity audit meanwhile.
