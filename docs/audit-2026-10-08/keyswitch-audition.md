# Keyswitch audition picker — W4

Base: published integration `3f49a9a109220c95546596e37204b5b2faa829b7`. Branch: `v2/fix-audition-picker`. This changes the shared scanner picker, not the source instrument or native engine.

## Instrument intent and cause

The installed Afflatus Chapter II Brass `2 Horns KS.nki` has 220 groups, 17,084 mapped zones and 11 script-owned articulation rows at MIDI 24–34. The detected script default is MIDI 24 (row 0); native group start criteria are empty, so KSP owns group selection. The script resets all 128 key types to `NI_KEY_TYPE_NONE` and colours to inactive, then recolours its playable range 36–77 as `KEY_COLOR_DEFAULT` without changing those key types. The script's saved playable bounds and keyswitch base were inspected in W0's existing private RAM cache while its quiet window was active; the native reader/compiled UI confirmed the same numeric layout afterwards. No script, authored text or sample data was exported.

The old picker treated the display type NONE as a hard MIDI veto, so all three gate conditions returned `pick=null`. That is NotAuditioned, not evidence of silence.

## Shared correction

Control keys and explicitly inactive colours remain excluded. Default/white/black playable colours survive a blanket NONE reset. The picker selects an enabled authored default articulation, or the first enabled source alternative, before querying the current native keyboard hints. It excludes source switch/control keys and chooses a mapped musical note with a covered velocity near 64. Existing numeric note plans cannot force an excluded control/switch key. The prepared IR and existing articulation model are unchanged. Numeric receipts now also expose the actual selected articulation.

The coverage scan remains bounded by 128 MIDI keys and two zone walks per key; it does not rescan the full map separately for every zone candidate.

## Evidence

`~/.cache/kontakto-fix-audition-picker/red.log`: the real NONE/default-colour regression fails before the correction. `final-targeted.log`: three targeted tests pass, covering display hints, inactive/control exclusions, sparse switches, velocity holes, and default/fallback articulation selection.

The retained one-cell source witness uses the shared audition and W6's signal graph: row 0, keyswitch 24, note 60/64. All 16 observed source zones match retained native zone/asset/group identities and key/velocity ranges. Audible amplifier/output lanes are native groups 8–15 under CC1=100 and CC11=127. The complete trace has 50,272 records and zero drops. RR is not scored as an exact take match. Numeric probe source/binary/receipt are retained in the W4 cache; the JSON trace is losslessly gzip-compressed with an integrity receipt.

Final cell receipts live under `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w4-audition-3f49-followup/final/`; before cells are in `~/.cache/kontra-runs/20261008T191303.775146Z-3f49a9a10922/`. The cold, product-warm and OS-warm before/after table and binary digest are captured in the follow-up summary. Timed runs are CONTENDED and provide no CPU/load/RSS performance certification. No extra full gate, release, tag or installation was performed.

`cargo test --profile ci --features shots --no-run` passed (`no-run.log`); shared scanner driver checks passed (`driver-check.log`). The final-source witness repeats all zone/asset/group and complete-trace assertions after the selection-order change. Final scanner SHA256: `c3b3ee5fbd2dfc13e680ad75e5a50ebd15fe963feff6fd39f0f2f93f3d9dc417`.
