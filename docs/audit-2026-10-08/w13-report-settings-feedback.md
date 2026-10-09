# W13 Report and Settings — feedback items 8–10

Status: SOURCE HOLD. Base aaf756dd. No Cargo jobs, runtime witnesses or after screenshots have run; source-only until the assigned machine turn. Crash delivery validation comes first.

Prepared UI changes:
- One Report tab combines selected-instrument results with retained journal events. Every entry expands inline to complete labelled details, wraps within the offered width and scrolls independently. Closed entries retain the bounded journal viewport and keyboard navigation.
- About, copy, export, refresh, log-folder and signal-trace operations remain compact actions. Filters open only on request. Existing background readers, export validation, private-evidence warnings and delivery receipts remain.
- Settings replaces the center workspace rather than adding a full-width strip. Library folders, Interface, MIDI / Output and Performance are separate sections; paths wrap; controls can wrap and the panel scrolls. The browser, keyboard, backend actions and persisted Settings schema stay unchanged. The spare typed-path field opens only on request and opens automatically without a folder picker.

W14 was notified before edits. Browser/library/cache/prefs source is untouched. Existing settings-body and folder action IDs remain; distill's old always-visible-input assertion now checks the explicit path action. Existing no-keyboard-overflow / preferences-scroll witness is retained in W14's browser_tests.rs.

The v1 source was inspected (0cb7a8a0:src/ui/{mod,header}.rs). It has the same full-width folder strip and Logs pane; this feedback fix does not claim to port a better v1 implementation.

## Pending validation

Failing-first source baseline: 49e1574b.
- ui::chrome_tests::report_is_one_tab_and_settings_uses_the_workspace
- ui::logs::tests::report_entries_expand_inline_and_close_without_a_permanent_detail_pane
- ui::load_report::tests::single_load_problem_expands_to_every_labelled_detail

Green regression set: those three, all ui::logs::tests (retention/filter/viewport/copy/export/privacy receipts), load_report_shot, empty_settings_uses_the_add_actions_as_instructions, settings_keeps_the_keyboard_inside_the_minimum_window, failed_load_diagnostics_remain_visible_without_an_instrument, and root CI no-run with shots.

After-shot fixture: KONTRA_REPORT_SETTINGS_SHOTS=<owned Windows receipt dir>, ui::chrome_tests::w13_report_settings_feedback_shots, shots feature. Capture 900×600 and 1180×780 once; inspect Report closed/expanded/About and Settings top/lower. User's attached screenshots are the before evidence. Native MUI; no HTML/CSS detector applies. No native visual-pass claim until actual captures are reviewed.

NEXT: crash red→green/no-run on machine turn, then UI witnesses/no-run and the assigned frozen CLAP cells; direct handoff to W11.
