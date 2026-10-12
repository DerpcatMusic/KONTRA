# moose-mui

[MUI](https://github.com/Matari-Audio/MUI) editors for moose plugins. The
moose port of MUI's `mui-truce`; enable it through the facade with
`moose = { features = ["mui"] }` and import from `moose::mui`.

- `MuiEditor` implements moose's `Editor`: a baseview child window under the
  host's, a wgpu surface on it, and one `Ui` frame per native event.
- `Bridge` binds widget ids to parameters. A drag, key step or click becomes
  the host's begin/perform/end; host automation or a state load is the value
  the next tree reads.
- `widget_id(param)` is the id `Bridge::bind` gives a parameter's widget.
- `HostScale` / `ParentWindow` turn moose's scale calls and parent handle into
  what `mui-baseview` opens with. The last host scale survives a reopened
  editor (MUI#48).
- `window` re-exports KONTRA's `kontra-native-host` through the stable
  `mui_baseview` dependency alias; `mui` is the toolkit itself.

```rust
use moose::mui::MuiEditor;
use moose::mui::mui::prelude::*;

fn editor(params: Arc<GainParams>) -> Box<dyn Editor> {
    MuiEditor::new(params, Ui::default(), (300, 200), |ui, bridge| {
        let gain = bridge.bind(ui, P::Gain, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0));
        col![gain, title(bridge.text(P::Gain))].pad(L).fill(Role::Surface)
    })
    .resizable((260, 180))
    .into_editor()
}
```

Build plugins with panics that unwind (`CARGO_PROFILE_RELEASE_PANIC=unwind
cargo moose build ...`): the window callbacks catch UI panics, which only
works when the cdylib unwinds.

## Dependencies

The MUI toolkit and public native bridges remain Git dependencies. This local
moose adapter uses KONTRA's distinct custom host package rather than overriding
upstream `mui-baseview`. The custom host retains KONTRA's wheel/pointer, dialog
parent and diagnostic hooks while consuming the upstream accessibility/IME
bridge. The root Cargo.lock pins MUI, and its MUI-source moose-baseview patch
provides the same native window types to both hosts.
