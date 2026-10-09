# W5 project persistence: three-library witness

Baseline: `54d9a5c5` (0.3.381), which already contains W11's DSP-owner persistence fix. The probe and getter fix are isolated commits above that baseline. No persistence schema or admission rule changes.

The opt-in host test uses the production `load_part`, native control ingress, render/effect processing, `Params::serialize_persist`, and `Params::load_persist`. It edits three controls in each library, compares every saved typed value (including every persistent array cell and text), and compares all native widget values and their roster after reopening. Decrypted library/script/sample bytes and saved state stay in RAM; diagnostics contain counts, indices, and hashes.

| Library | Persistent variables | Saved typed values | Native UI values | RED persistent losses | RED UI losses |
| --- | ---: | ---: | ---: | ---: | ---: |
| Conflux | 257 | 4,438 | 434 | 0 | 0 |
| Analog Strings | 599 | 8,337 | 1,010 | 0 | 0 |
| Dolce, 7 1st Violins Sustained | 575 | 1,282,218 | 690 | 240 | 0 |

Dolce's five affected persistent arrays each lost 48 cells. Their assignments cache `get_engine_par` for ATTACK, ATK_CURVE, DECAY, SUSTAIN, and RELEASE. Restore callbacks replaced the saved values with zero because inactive native AHDSR slots had no live DSP binding. The reader had retained their authored physical addresses and values in the IR, but lowering discarded those values. Already 195 cells differed immediately after restore; all 240 differed after 64 render blocks.

V1's persistence path (`src/ksp/runtime.rs:restore_persistent`/`persistence` and `src/plugin.rs:persisted`) saves and restores named persistent values. The current typed host path already captures those values plus widget and DSP owners; the recorded failure occurs after successful restore, inside a getter-driven callback. A broader persistence port would not address that loss.

The fix ports the live-then-authored read fallback from v1 `0cb7a8a0:src/ksp/calls.rs` and the sorted lookup from `src/ksp/runtime.rs`, adapted to the addressed service. Lowering retains the imported values in a validated, sorted, read-only table. Live DSP owners remain authoritative; unknown addresses and writes without DSP owners remain errors. There is no successful mirror write. The synthetic tests verify the inactive read, rejection of writes/missing addresses, live-owner precedence, and allocation-free operation.

Validation receipts: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w5-project-persistence/`. `attribution.log` is the failing exact-381 run; `source-attribution.log` attributes the five loss classes without saving script text. GREEN: all three libraries have zero persistent and UI losses after the 64-block callback settlement. Dolce still has 195 differing cells in the diagnostic snapshot taken immediately after load, before scheduled callbacks settle; immediate-save behavior is not established by this gate. Core lowering: 31/31; host persistence: 8/8 (corpus test ignored in that filtered run); explicit corpus round trip: 1/1; core/KSP/host `cargo test --no-run`: PASS. The failing corpus test changed from 0 pass / 1 fail to 1 pass / 0 fail.

Run the corpus test through `kontakto-heavy`, with `KONTRA_PERSISTENCE_CORPUS=/mnt/MAIN_STORAGE/Libraries/Kontakt`:

```text
cargo test --locked --profile ci -p kontakto --features shots --lib corpus_project_save_reload_compares_every_persistent_and_ui_value -- --ignored --nocapture
```

This is a production host-path round trip, not an actual DAW session or native Kontakt comparison. It covers the three named instruments, not every instrument in the corpus. It does not add DSP consumers for inactive modules.
