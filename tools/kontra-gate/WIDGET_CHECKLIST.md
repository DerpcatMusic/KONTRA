# Widget acceptance checklist

Original view follows the running library script: page dimensions, control
positions, parent offsets, visibility and z-order. Off-page controls do not grow
the page. Controls that overlap a page edge retain their authored position and
are clipped to that page. No inferred panels or substitute layout.

Run each applicable obligation for every visible, enabled interactive target in
Kontakt and Falcon libraries. Record source/binary identity, item/program/page,
condition, target identity hash, gesture, passed/total, and typed failure reason.
A source test alone does not certify the library or native-host acceptance axis.

| Obligation | Required witness |
| --- | --- |
| Drag direction | Positive/negative movement follows the scripted axis and orientation; perpendicular motion does not alter the value. |
| Drag speed | Measured value delta follows scripted sensitivity and remains consistent at editor/device scales; closed drags return to their start. |
| Fine modifiers | Each supported platform modifier follows Kontakt/Falcon semantics and produces the expected smaller delta without a jump. |
| Double click | Reset to the authored default for reset controls; value/text edits enter editing where authored. |
| Mouse wheel | Both directions, fine modifiers, steps, bounds and popup wheel capture work without changing a control underneath. |
| Menus | Open, select every enabled choice, preserve item values/order, reject disabled choices, dismiss and navigate by keyboard. |
| Buttons and switches | Latching, momentary, press/release, multistate and authored callbacks follow script semantics. |
| Value display | Text, units, ratios, precision, fonts, authored labels and filmstrip frames reflect the current value during and after input. |
| Automation | Host edits reach the same engine/script parameter; script changes update display and host readback; automation identity survives reopen. |
| MIDI learn | Learn starts, binds the intended port/channel/controller, updates the same target, cancels/replaces correctly and recalls its assignment. |
| Other controls | XY axes, table cells, label/text editing, MouseArea events, file selection and Native/UVI controls receive their actual input paths. |
| Causal value and sound | Accepted target input changes its owning script/engine parameter; relevant audition confirms the expected signal-graph change. Background script changes cannot certify input. |
| Save and fresh reopen | Save after the gesture, destroy/recreate the plugin, load state, verify owning values, UI state, automation/MIDI assignment and relevant sound. GUI-only reopen is insufficient. |
| Faults and fluidity | No script/render/audio faults; smooth input and fast first frame need native presentation and timing receipts under the declared machine condition. |

Unimplemented or unmeasured obligations remain UNKNOWN. Navigation-only controls
need a page/visibility-state witness instead of a parameter delta. Disabled and
passive controls have no edit obligation. Frozen v1 source and binaries are the
local comparison reference; Kontakt/Falcon interaction calibration still needs
an explicit native witness before claiming faithfulness.

Use source-specific conventions when testing modifiers and numeric entry:

| Source | Fine drag | Reset | Numeric entry |
| --- | --- | --- | --- |
| Kontakt | Shift | Ctrl-click on Windows; Cmd-click on macOS | Double-click a value field |
| Falcon | Ctrl on Windows; Cmd on macOS | Alt-click on Windows; Option-click on macOS | Double-click a numeric control; Enter confirms and Escape cancels |

These conventions come from the official [Kontakt interface manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-player-manual/en/user-interface-elements)
and [Falcon interface manual](https://manual.uvi.net/falcon/en/interface/).
They define separate test obligations; support remains unverified until the
actual input path has a witness. Authored script behavior still governs custom
controls. Shared KONTRA modifiers must not silently replace Falcon semantics.
