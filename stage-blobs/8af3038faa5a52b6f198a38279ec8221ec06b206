# W13 Report and Settings — feedback items 8–10

Status: **GREEN / READY**. Base `aaf756dd`; validated source `1f2eeb5157fdcec778668c2af62a5cc0ebb3a525`. Three baseline failures reproduced, 11 focused checks passed, root `cargo test --profile ci --features shots --no-run` passed, and twelve native CPU captures were reviewed at 900×600 and 1180×780. Every Cargo call used `kontakto-heavy` sequentially. No release, install or timing run.

- One Report tab combines selected-instrument results with retained journal events. Entries expand inline to complete labelled details, wrap within the viewport and scroll independently. Closed entries keep the bounded journal list and keyboard navigation.
- About, copy, export, refresh, log-folder and signal-trace operations remain compact actions. Filters open on request. Existing background readers, export validation, private-evidence warnings and delivery receipts remain.
- Settings replaces the center workspace. Library folders, Interface, MIDI / Output and Performance have separate sections; paths and controls wrap, and preferences scroll while the toolbar and keyboard stay visible. Existing settings schema and backend actions remain. Typed-path entry opens on request or automatically without a folder picker.

W14 was notified before edits. Browser/library/cache/prefs source is untouched. Existing settings-body and folder action IDs remain. The old always-visible-input assertion now checks the explicit path action; the existing keyboard-fit/preferences-scroll witness remains.

v1 source `0cb7a8a0:src/ui/{mod,header}.rs` has the same old folder strip and Logs pane, so this feedback change is not a v1 port.

## Validation

Receipt directory: `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w13-report-settings-runtime-20261009/`.

| Source / receipt | Result |
|---|---|
| `49e1574b`, `RESULT.json` | Three RED witnesses: separate tabs, permanent detail pane, non-expandable single error |
| `5fd771f5`, `RESULT.json` | Workspace GREEN |
| `RESUME.json` | Wrapper refusal at 14G target-volume space; no tests ran |
| `5fd771f5`, `space-retry/RESUME.json` | 10 PASS / 1 FAIL: journal expansion consumed its own wheel before scrolling |
| `1f2eeb51`, `scroll-fix/RESUME.json` | 11 PASS / 0 FAIL; root shots no-run PASS |
| `scroll-fix/shots/` | Twelve reviewed captures: Report closed/expanded/scrolled/About and Settings top/lower at both sizes |

The scroll fix `861e8c0d` uses the existing popup boundary pattern: the scroll node receives the wheel first, and its enclosing boundary prevents propagation to the list behind it. Both journal and instrument details use it. The original journal regression remains; `1f2eeb51` additionally checks actual instrument-detail scrolling and a stationary outer list in the capture fixture. Failed runs are retained.

Focused coverage includes load-report grouping/single-error expansion, journal detail expansion/filtering/virtualized history, crash receipts/public-link copy, empty Settings typed-path access, keyboard fit/preferences scrolling, failed-load visibility, and native captures. Existing compiler warnings remain.

These are synthetic native MUI interactions and CPU-rendered scenes, not presented DAW-window or live CLAP/VST3 evidence. The full integration gate stays with W0; CLAP timing requires a separate quiet slot.

Merge the complete `aaf756dd..HEAD` UI stack from `v2/w13-report-settings`; no W0 worktree, branch or cache was written by this validation.

NEXT: W0 integrates this UI stack; direct drained quiet handback to W9, then W6 per coordinator order.
