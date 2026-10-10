# Mapping overlap keyboard navigation — next-batch source only

## Custody and scope

Checkout: `/mnt/Windows11/DEV_WORKSPACE/kontra-worktrees/ui-mapping-focus-next-20261010`.
Branch: `v2/pi-ui-mapping-focus-next-20261010`.
Base: **429e7dff3ecb0f53aef32c43201ab796ccc8c28c**, independently resolved locally before creating this clean checkout.

1. **9b7c11370cc3593e53a43841efe35d79ec0f2a9a** — behavioral regression checkpoint. Expected failure on the first Down-key focus assertion; not compiled or executed here.
2. **2f9e018843c8fe4f6aff762d871d69b53caf823a** — overlap navigation implementation and bounded idle/foreign-focus allocator regression. Rust execution pending.

This completes one narrow KONTRA UX extension: a clicked/focused overlapping-source row can select and reveal another source with Up/Down, Home/End or Page Up/Down across every existing 32-row page. Page keys move by the visible rail height, not by an arbitrary source batch. Enter and pointer activation remain on the existing house row. The inspector and waveform keep using the actual selected IR source ID. No zone editing, fake status, sample decode, new parameter, dependency, worker, retained owner, or DSP/parser change is included.

The previous delivered UI checkout/stack and the integration checkout are unchanged. The large W4 navigation/index/strip stack was inspected but not imported. Group/articulation rail keyboard traversal, virtualized worker metadata, duplicate Sound/Mapping navigation, Settings controller relocation and persistent Root/Tune/Gain/Pan editing remain separate unfinished slices.

## Pinned implementing paths

All following source locations are pinned to **2f9e018843c8fe4f6aff762d871d69b53caf823a**:

- `src/ui/mapping.rs:116–127`, `stack`: preserves ascending source-zone identities after group/key/velocity filtering. The new navigator uses binary search on that existing ordered stack, not contiguous source ordinals.
- `src/ui/mapping.rs:197–242`, `STACK_PAGE`, `STACK_ROW`, `walk_stack`: accepts only the current slot's real source-row focus; ignores modified keys and unrelated/text/stale focus. Bounded cursor movement updates one selection scalar, requests the matching row focus and reveals its page-relative offset through the existing MUI scroll API. The idle path adds no source traversal or storage owner.
- `src/ui/mapping.rs:538–546`, `view`: navigation occurs before selected-map outline and inspector construction. Existing 32-row paging and 22px row height share the same constants as the keyboard path.
- `src/ui/mapping.rs:632–686`, overlap rows: unchanged source label, house `interactive` row, focusability, accessibility toggle and activation path. Only the shared page/height constants change.
- `src/ui/mapping.rs:873–900`, `inspector`: still resolves `source_ids[selected]`, part generation and the existing `part.zone_waveform` provider. The navigation adds no alternate decode/readback path.
- `src/ui/mapping.rs:1150–1198`, `mapping_overlap_walk_idle_and_foreign_focus_do_not_allocate`: actual helper, existing thread-local allocator, 128 repeated calls per focus case plus foreign/stale-key checks.
- `src/ui/v2_tests.rs:5566–5731`, `mapping_overlap_keyboard_walks_filtered_sources_across_pages`: actual Shared view → Bridge → editor Harness route. A 193-zone fixture filters to 97 non-contiguous identities. It checks pointer-derived row focus, key boundaries/page crossings, selected inspector, focus-visible state, scroll bounds, modified keys, Enter, search focus ownership, no audition events and unchanged IR at 900×640 and 1180×780.

House input contract at the same SHA: `vendor/mui/src/ui/input.rs:180–195` allows focus on a row being built and supplies only its focused key stream; `vendor/mui/src/ui/scroll.rs:114–124` clamps requested scroll against the **new** page's content. These source contracts informed the implementation; they are not runtime evidence.

## Native specification check and own-v1 comparison

Used immutable authoritative native-spec receipts as the allowed equivalent evidence check. Their hashes were independently reverified. No native app or REA binary session was opened; no library/sample/key payload was read or exported.

- [NI Kontakt user guide, Classic view → Mapping Editor](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view): the Mapping Editor represents Zones referencing samples, key/velocity ranges and selected-zone parameters. The selected Sample Field is a filename, not a substitute sample assignment. Snapshot SHA256 **846a09243ad7d17d8daa92bf5105418501ebb25cd358ba8d09108ffb5d4ae7c4**, retrieved 2026-10-10T02:13:00.767464Z. Relevant exact headings: **Mapping Editor**, **The Control Strip → Sample Field**. This supports preserving selected-source identity and read-only metadata. It does not establish Kontakt's overlap-list keyboard implementation.
- [UVI Falcon manual](https://cdn.uvi.net/UVIFC_Falcon/manuals/Falcon_manual_en.pdf), printed pp.36–37, **Interface → Main → Edit → Mapping**: keygroups occupy key/velocity blocks; the toolbar shows the selected keygroup's root/ranges, and native editing is a separate operation. Snapshot SHA256 **32fe8169a2efc4a40409780129a79fb92ef7a3d8ceeb43c524791929702fe4fe**, retrieved 2026-10-10T02:13:01.626399Z. Saved text lines1200–1272 provide the inspected spans. This slice does not implement Falcon's editable fields or claim those controls work.

Original snapshots and URL/provenance manifest: `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/official-docs/`.

Own frozen v1 **0cb7a8a0b4d43086596a64c77320caa1b26d6d98**, `src/ui/browser.rs:1093–1161`, `walk`, already uses bounded Up/Down/Page/Home/End row navigation and separates text focus. The new slice retains that product convention using the current house MUI widgets, without copying the old allocated key-vector path. V1's 27-file UI tree has no `src/ui/mapping.rs` or `src/ui/inside.rs`; module absence alone does not prove feature absence. No v1 runtime/performance comparison was made.

**Native keyboard parity: UNKNOWN.** This is KONTRA-specific navigation, not a vendor-authored script builtin or a roadmap parity completion. Native behavior is documented only at the concepts described above.

## Checks performed and pending validation

- `git diff 429e7dff --check`: PASS.
- `rustfmt --edition 2024 --config skip_children=true --check src/ui/mapping.rs`: PASS; syntax/format only, not typechecking.
- New root fixture parsed with rustfmt. Unrelated baseline formatting was restored; only the new fixture remains changed in `v2_tests.rs`.
- Python cursor/scroll arithmetic model: **224,868 comparisons PASS**. Exhaustive lengths1–193 plus bounded1024/54k edges, non-contiguous source identities, full/partial pages and 22/44px rails. This does **not** execute Rust, MUI, PCM or a native host.
- Incumbent historical synthetic Mapping screenshot at `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w4-settings-parity/screenshots/mapping-900x640.png` inspected for existing rail hierarchy. It is not a current/new-source visual receipt.
- Graft ask/grep/skeleton/callers used before edits; exact branch source spans and shared MUI contracts checked where graph edges were absent.

Evidence: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-ui-mapping-focus-next/SOURCE-MANIFEST.json`, SHA256 **e550e22f33f19fb798b8213d418059fea57aa277a54b6d6323170dddd19a6814**. This binds product source, model files, native-spec snapshots and separately verified prior-batch receipts.

No cargo/rustc/clippy/build, executable run, official host, Wine, install, publication, schedule, nested agent or dev server was launched by this worker. No CARGO_TARGET_DIR/RUSTC_WRAPPER override was set.

Integration alone should validate the next frozen combined source. Required filters:

- `ui::v2_tests::mapping_overlap_keyboard_walks_filtered_sources_across_pages`
- `ui::mapping::tests::mapping_overlap_walk_idle_and_foreign_focus_do_not_allocate`
- Existing `mapping_navigation_reaches_late_takes_and_restores_fitted_keys`, `mapping_sound_tabs_preserve_zone_identity_and_ir`, `mapping_group_search_keeps_selection_and_offers_empty_recovery` and Mapping unit neighbors.

Return actual counts/source SHA/artifact SHA/log hashes. Use `KONTAKTO_MAPPING_SHOTS` with an integration-owned receipt directory for the prepared two-size screenshots; review actual key focus and clipping. No per-lane build cycle is requested. A regression-only RED run on9b7 is optional and must fit the coordinator's combined plan, not trigger an extra worker build.

## Prior frozen batch result — distinct from this candidate

Independently reverified all four current UI logs and exact artifact hashes in `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/pi-integration-round2/test-results.json`:

- Source **429e7dff3ecb0f53aef32c43201ab796ccc8c28c**, tree **94ad5caaa5e5ec70e1595d34166c25110d294c67**, manifest SHA256 **69a158bfb771a8853c89ddd2a00fa5f39e326a147bb1deedb120c18cb338a29c**.
- Root artifact SHA256 **54b9f1bea9eb3c9f800a6141bf544f9c7f9abff560136ac130f5a5b784122abc**.
- Session: **3 passed**, including actual warmed64×five-typed allocator fixture and legacy component.
- Native publication: **7 passed,4 ignored**, including State seed warmed allocator fixture.
- Browser: **21 passed,1 ignored**, including protected-status geometry and two-size captures.
- Shared UI IR: **7 passed**, artifact SHA256 **9de64b6de9a0c96136cfc6a6f0249130ab5315c44ac4af6278055f3b9d86d7b8**.
- `test-results.json` SHA256 **3cbe70e59d9334b3cb1b7c529252f121c3b9d6f446f04cdb3981f9afc9005c4a**; `compiled-artifacts.json` SHA256 **a47f7877f84520ab11adb9069d98f03e5db52cda9dbcc44798d47c9799413ac8**.

The coordinator manually inspected synthetic-owned browser captures. Caption/title and access cause are separate, but both have a partly clipped large/faint **Owned** backdrop below the small title. Cause is **not diagnosed**. Capture hashes:900 **4719f5017c1763980ebd3fdeca3822f7fbf13dcd8fb7cdb807cff074b00bbf5e**;1180 **c961f7317eeadca790d7d5046fd87ccc115bbca099240f2bf66f64329f9debfc**. That visual finding stays open for a following browser slice. Geometry PASS is not full visual-polish PASS.

These results validate the bounded readback/helper and browser fixtures only. They do not validate this new Mapping source, universal zero allocation, native-host parity, the Conflux freeze/leak hypothesis or lower CPU AND RAM than both frozen v1 and Kontakt. The global performance acceptance remains **UNACHIEVED**.

NEXT: coordinator assembles this two-commit source slice and this receipt in the following combined manifest; integration runs the actual Mapping regressions. Keep group-rail navigation, real persistent zone controls and the browser backdrop finding explicitly open.
