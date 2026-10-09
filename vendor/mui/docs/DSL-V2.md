# DSL v2 and the 2026 cleanup

The spec every change in this pass follows. Where a builder has to choose a
name not listed here, it follows the principles, adds a migration rule, and
records the name in the "Decided during implementation" table at the bottom.

## Before / after

A knob with a readout and an icon, in the v1 DSL:

```rust,ignore
// before
let (knob_el, changed) = knob(ui, "gain", v, 0.0..=1.0);
column([
    row([
        icon(icons.clone(), sym::VOLUME_UP).text_size(20.0),
        knob_el.size(L).el().tip("Gain"),
    ])
    .pad_xy(16.0, 8.0)
    .border(Role::Dim, 1.0),
    leaf(dot, dot).pill().fill(Role::Primary)
        .anchor(Align::Center, Align::Center).offset(0.0, 0.0),
    label(caption).fill(Role::Dim).min_width(120.0).expand(),
])
.gap(Xs)
```

and now (this block is compiled by `cargo test --doc -p mui`):

```rust
# use mui::prelude::*;
# let (mut ui, mut v, dot, caption) = (Ui::default(), 0.5, 6, "-6 dB");
# let ui = &mut ui;
let gain = knob(ui, "gain", "Gain", &mut v, 0.0..=1.0).size(L).tip("Gain");
let changed = gain.changed;
col![
    row![icon(sym::VOLUME_UP).text_size(20), gain]
        .pad((16, 8))
        .stroke(Role::Dim)
        .stroke_width(1),
    block(dot, dot).pill().fill(Role::Primary).centered(),
    body(caption).fill(Role::Dim).min_w(120).grow(1),
]
.gap(Xs)
# ;
```

## Principles

1. **One way to say a thing.** No alias verbs, no parallel constructor and
   macro names. If two builders do the same thing, delete one.
2. **Names say what you see, not how it is implemented.** `block`, not `leaf`;
   `a11y`, not `role(Kind)`.
3. **No trait imports for the core vocabulary beyond the prelude.** Layout
   verbs are inherent on `Node`; paint verbs on `Paints` (shared with `Style`);
   everything else on `Styled`. `Sugar` and `IntoLen` are gone.
4. **Switches take no arguments.** `.focusable()`, `.disabled()`, `.clip()`.
   Conditional use goes through `.when(cond, |e| e.disabled())`. The one
   exception is a value that is *inherently* dynamic data, never a switch.
5. **`_with(x)` is the only way to pass an explicit option to a verb that has
   a default.** `.animate()` / `.animate_with(spring)`.
6. **Ids are `Id`, everywhere.** Every API that names a node takes
   `impl Into<Id>`; every map keyed by a node is keyed by `Id` (or its hash).
   Names starting with `/` are reserved for the runtime; `Id::of` rejects them
   with a debug assertion and `Id::is_named()` is public, the one copy of the
   rule.
7. **Widgets return named results.** No tuples, no `.0`.
8. **Renderer choice never lives in the tree.** Backends are chosen by the
   `SceneSpec` or the host.
9. **Every rename ships a migration rule** in `tools/mui-migrate`, and the
   repo's own examples/tests/preview are migrated *by running the tool*, not
   by hand. That is the tool's test.

## Vocabulary changes

### Constructors (mui-scene / mui-layout)

| old | new |
|---|---|
| `leaf(w, h)` | `block(w, h)` |
| `column(children)` | `col(children)` |
| `overlay(children)` | `stack(children)` |
| `row`, `grid`, `fits`, `spacer`, `text`, `icon`, `canvas` | unchanged |
| `canvas_cached(key, f)` | `canvas_keyed(key, f)` |
| macros `row! col! stack! grid! fits!` | unchanged (now match the functions) |

### Text roles

Sizes come from a new `Theme::type_scale` (`TypeScale { title, body, caption }`,
defaults 18/13/11) read at resolve time, not hard-coded px.

| old | new |
|---|---|
| `title(s)` | `title(s)` |
| `label(s)` (13 px text) | `body(s)` |
| `caption(s)` | `caption(s)` |

### Layout sugar (inherent on `Node`, moved from `Sugar`)

`w`, `h`, `square`, `center`, `start`, `end`, `between`, `full` move to
mui-layout as inherent methods. `Len` gets `From<i32>`, `From<f64>`,
`From<f32>`, so `.width(120)` and `.w(120)` both compile; `IntoLen` is deleted.

| old | new |
|---|---|
| `.anchor(ax, ay).offset(dx, dy)` for absolute placement | `.at(dx, dy)` (start/start anchored) — `anchor`/`offset` stay for the general case |
| `.anchor(Align::Center, Align::Center).offset(dx, dy)` | `.centered_at(dx, dy)` |
| `.min_width(w).min_height(h)` pairs | unchanged, plus `min_size`/`max_size` taking `impl Into<Size>` from `(f64, f64)` |

### Accessibility

| old | new |
|---|---|
| `Kind` (a11y enum) | `A11y` |
| `.role(Kind::X)` | `.a11y(A11y::X)` |
| `.label(name)` (accessible name) | `.named(name)` |
| `Semantics` | unchanged |

`Role` (the colour role) keeps its name: with `Kind` gone it no longer
collides. `pub use Role::*` leaves the prelude; write `Role::Raised`.
The spacing tokens `Xs S M L Xl` stay in the prelude — they are the compact
core of the DSL.

### Switches

| old | new |
|---|---|
| `.disabled(bool)` | `.disabled()` (+ `.when`) |
| `.scroll_bar(bool)` | `.scrollbar()` (off by default) or `.no_scrollbar()` if on by default — keep whichever matches today's default |
| `.focusable()`, `.baseline()`, `.captures_wheel()`, `.tracks_pointer()`, `.clip()`, `.scroll()`, `.float()`, `.sticky()`, `.wrap()` | unchanged |

### Motion

| old | new |
|---|---|
| `.animate()` | `.animate()` |
| `.transition(spring)` | `.animate_with(spring)` |
| `.animate_layout()` | `.animate_layout()` |
| `.layout_transition(spring)` | `.animate_layout_with(spring)` |

### Weld and material

One verb, options on the value:

| old | new |
|---|---|
| `.weld_with(w)` | `.weld(w)` |
| `.weld_shape()` | `.weld(Weld::shape())` |
| `.weld_borders()` | `.weld(Weld::borders())` |
| `.weld_morph(p)` | `.weld(Weld::default().morph(p))` |
| `.weld_quality(q)` | `.weld(Weld::default().quality(q))` |
| `.without_weld()` | `.weld(Weld::off())` |
| `.exclude_from_weld()` | `.unwelded()` |
| `.gpu_weld(w)` / `.reference_weld(w)` | `.weld(w)` + `SceneSpec::weld_backend(WeldBackend::..)` |
| `weld_morph![p; ..]` | `weld![Weld::default().morph(p); ..]` |

Material/surface verbs (`union`, `join`, `cut`, `keep`, `join_border`,
`inset_surface`, `inset_surface_of`, `surface_layout`, `shell`,
`border_ramp`) move with the material code into the new `mui-material` crate
as the `Material` extension trait (re-exported by the prelude). Renames:

| old | new |
|---|---|
| `.join()` (squares children, clips ends) | `.segmented()` — and it must also square children pushed *after* it (resolve-time, not build-time) |
| `.cut(el)` / `.keep(el)` on a leaf | compile to a wrapping `stack` instead of a silent no-op |

### Identity and cross-references

`.id(..)`, `join_border(..)`, `inset_surface_of(..)`, `glide` callbacks all
take `impl Into<Id>`. A dangling cross-reference names the missing id in its
error. Icons: `mui_symbols::sym::HOME` generated `char` consts replace
`material_symbols::codepoint("home")`; the table moves to its own
`mui-symbols` crate with a sortedness test.

### Resolving

`resolve_scene`, `resolve_scene_with`, `resolve_scene_cached`,
`resolve_scene_animated`, `resolve_scene_retained` →
one `Resolver` that owns the caches:

```rust,ignore
let mut r = Resolver::new();          // caches live here
let scene = r.resolve(&spec)?;        // &ResolvedScene, kept for the next call's memos
let scene = r.resolve_after(&spec, glide, prev)?; // a runtime that keeps its own scenes
resolve(&spec)?                       // free fn for the uncached one-shot
```

### Style merging

`Style` fields that "may be unset" become `Option<T>` so `over` is
`other.x.or(self.x)` for every field. `Radius::Theme`, `CornerStyle::Round`,
`Fill::None`, blur `0` stop meaning "unset". `union` becomes `Option<bool>`.
`Elevation::shadows()` returns `&'static [Shadow]`.

### Widgets (crate `mui`)

Every widget returns `#[must_use] Response<C = bool>`:

```rust,ignore
pub struct Response<C = bool> { pub el: El, pub changed: C }
```

`C` is `bool` for value widgets and `Option<Edit>` for curve/bins. Controls
return `Response<bool, Control>` (the struct has a second parameter,
`E = El`): `.el` is the `Control`, still taking its look, so the caller never
reaches into `.0`. Every widget
takes `id: impl Into<Id>`. Positional bools (`color_picker(.., alpha)`)
become option structs or builder methods. `Ui::state` returns
`Interaction { hover, press }`. `bins` and `curve` agree on one mutation
model (`&mut` value in, `changed` out).

`mui-truce::Bridge::bind(ui, P::Gain, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0))`
— the id is derived from the parameter and handed to the closure, so it
cannot be typed twice. `bind_bool` exists for toggles.

## Structure changes

- Edition 2024, `rust-version`, `[workspace.lints]`, `[workspace.dependencies]`
  (every internal crate + shared external versions).
- `mui-stage`, `mui-reel`, `mui-motion-bridge` and the plugin adapter tools move to
  `media/`, a separate workspace depending on `../crates/*` by path.
- New crates: `mui-material` (split from mui-scene), `mui-symbols`.
- Spacing tokens move from mui-geometry to mui-layout.
- kurbo is the one `Point`/`Rect`/`Affine`/`BezPath`; mui-weld and
  mui-geometry drop their copies.
- `mui::Ui` splits into `ui/{mod,input,scroll,drag,motion,memo}.rs` + `ui/tests`;
  per-key maps merge into one `Id`-keyed `NodeState` map.
- `Walk` splits its state; dense pre-order maps become `Vec`s.
- One shared GPU host (`mui-vello::host`) and one keymap for preview and truce.

## Decided during implementation

| old | new | why |
|---|---|---|
| `'/'` checks by hand | `Id::is_named(&str)` (associated fn), `Id::runtime(name)` for reserved ids | the rule is checked on keys that are `&str` (a11y tree, motion bridge), so no `Id` has to be built first |
| `glide` callback key | `&Id` | the scene keys nodes by `Id` (a tree path is `Id::runtime("/0/2")`, inline, no allocation), so the key is handed over as stored; the interned `Arc<str>` per node is gone |
| `resolve_scene_animated(spec, dt)` / `_retained` | `Resolver::resolve_after(&spec, glide, prev)` | the animated path needs the glide callback and the previous scene, not a `dt`; plain `Resolver::resolve` keeps its own previous scene |
| `TextCache` | `Resolver` (`recycle`, `layout_stats`, `text_runs` delegate); the text cache is crate-private | one public cache type; `Resolver::welds` stays public for weld stats |
| `.without_weld()` | `Weld::off()` = fill and border both `Keep`; `.weld(Weld::off())` clears the weld | an off weld is a value, not a separate verb |
| `.gpu_weld(w)` backend | only `SceneSpec::weld_backend(..)` (and `Ui::gpu_welding` in `mui`) | principle 8 |
| `material_symbols::codepoint("10k")` | `sym::_10K` | a name starting with a digit gets a `_` prefix to be an identifier |
| dangling-ref errors | `SceneError::MissingId { what, id }` | one variant for `join_border`, `inset_surface_of`, border-ramp anchors |
| widget renames | `mui-migrate` widget rules on by default (`--no-widgets` opts out); docs mode on by default, `--no-docs` turns it off; a code block with a `// old` / `// before` line is left as written | the widget API exists now; before/after examples document the old API on purpose |
| `Control` in a `Response` | `Response<C = bool, E = El>`; controls return `Response<bool, Control>`, everything else `Response<C>` with an `El`; `Response: IntoEl` | the look (`.size`, `.variant`, `.role`, `.value_text`, `.px`) stays a builder on `.el` instead of a positional argument on every call; a `Response` drops into `row![..]` whole |
| `Bridge::bind` closure `-> El` | `-> impl IntoEl`: `\|ui, id, v\| knob(ui, id, "Gain", v, 0.0..=1.0)` | no `.el` to type; a hand-built tree still works |
| widget argument order | `ui, id, label (controls), &mut value, range / options`; `toggle` and `drag_value` gain a label (their accessible name, not drawn) | every control is named for a screen reader; `button`/`slider`/`knob` already took one |
| `color_picker(.., alpha: bool)` | `ColorOpts { alpha }` (`Default` is opaque) | the `TextOpts` shape |
| `bins` mutation | `&mut Bins`, `Bins::authored: &mut [f32]`; paint, reset and select are applied (a paint moves the selection); `BinEdit` reports what | `curve`'s model: `&mut` value in, what changed out |
| `stepped(ui, id, v, &range)` | unchanged | a keyboard helper for custom controls, not a widget |
| `Ui::state` | `Interaction { hover, press }` (in `mui`, apart from `mui_input`'s pointer `Interaction`, which `ui.rs` imports as `Pointer`) | |
| `mui` surface | the prelude lists every name (no glob); the six crates stay as named modules, `mui::scene` .. `mui::vello`, each its crate's own curated `lib.rs` | nothing arrives in the prelude because a crate underneath grew it, and no downstream path changes |
| `one_line` / `many_lines` args | a `Layers` struct (shown text, caret, selection, preedit, blink, line height) | the `too_many_arguments` expects are gone |
| `.disabled(bool)` rewrite | only on builder chains in mui files | `Palette::disabled(color)` has the same shape |
| size budget | `Element` ≤ 336 B, `El` ≤ 664 B (were 360 / 688) | eight switches are one `flags: u16` (`Element::FOCUSABLE` .. `SCROLL_BAR_OFF`, read with `has`), and the scrollbar heat, runtime state, moved to `SceneSpec::scroll_bars` |
| playground DSL `leaf` / `join` | `block` / `segmented` | the text DSL follows the Rust names |
| `.cut`/`.keep` on a leaf | pushing a child onto a `block` turns it into a `stack` of its own size | covers `cut`, `keep` and any push, no wrapper node |
| motion-bridge key names | `" "` stays `Key::Space`, one char is `Key::Char`, else `Key::from_name` | keeps the editor's existing spelling |
| material verbs in `mui-material` | the `Material` trait and capture (`Capture`: `isolate`, `without`, `capture_layers`; `resize_capture`) move; surfaces, regions, border ramps, material welds, partition and the GPU weld hand-off stay in mui-scene | the resolve half reads and writes the walk's frames, paint list and `Resolver` caches at a dozen points mid-walk, and its data (`Extras::border_ramp`, `SceneSpec::weld_backend`, `ResolvedScene::external_welds`) is stored in mui-scene types; a hook would be a dozen-method trait object for one implementation |
| number arguments | lengths (`block`, `w`, `h`, `size`, `square`) take `impl Into<Len>`; every other number in the builder surface (`at`, `offset`, `centered_at`, `min_w`, `grow`, `shrink`, `flex`, `basis`, `aspect`, `scrolled`, `min_col`, `text_size`, `stroke_width`, `backdrop_blur`, `step`, `pct`, `cq`, `clamp`) takes `impl Px`, a one-method trait on `f64 f32 i32 u32 usize`; `Len`, `Spacing` and `Radius` convert from any of them | `.at(8, 4)` and `block(28, 28)` compile; fractions that are not pixels or weights (`opacity`, `icon_fill`, `grade`) stay `f32`. Widening, so no rule |
| `.width(l)` / `.height(l)` | `.w(l)` / `.h(l)` (with `size`, `square`) | principle 1; the short names already won in the examples |
| `.min_width(x)` / `.min_height(y)` | `.min_w(x)` / `.min_h(y)`; `min_size`/`max_size` unchanged, no `max_w`/`max_h` (nothing needed them) | reads with `w`/`h` |
| `.expand()` | `.grow(1)` | `grow` states the weight; with `impl Px` the `1.` is gone |
| `.pad_xy(x, y)`, `.insets(i)` | `.pad(p)` with `p: impl Into<Pad>`: a number or token (all sides), an `(x, y)` pair, or `Insets` | one verb for padding |
| `.border(paint, w)`, `.no_border()` | `.stroke(paint).stroke_width(w)`, `.no_stroke()` | `stroke` alone recolours without touching the width (the state overrides need that), so it stays and the combined verb goes; the verbs now match `Style::stroke` |
| `.apply(f)` | `.when(true, f)` | `when` is the general form; `apply` had no callers |
| `.centered_at(0., 0.)`, `.anchor(Center, Center)` | `.centered()`; `.centered_at(dx, dy)` stays for a nudge | `centered` places the node in its parent, `center` still centres its children |
| placement offsets | `at`, `offset`, `centered_at` take `(dx, dy)` as two `impl Px`; `Appear::Slide(dx, dy)` is an enum value and stays `f64`; `Vec2` is for geometry and pointer data only | one shape for every placement verb |
| widget look and element verbs | `Response<C, Control>` forwards `size`, `variant`, `role`, `value_text`, `px`; any `Response` takes the common element verbs (`tip`, `named`, `disabled`, `when`, `fill`, `opacity`, `animate`, `appear`, `w`, `h`, `square`, `min_w`, `min_h`, `grow`, `shrink`, `basis`, `flex`, `align_self`, `pad`, `at`, `offset`, `centered`, `centered_at`, `span`, `order`), which finish a control into an `El` and keep `changed`; one `forward!` macro lists them | `knob(ui, id, "Gain", v, 0.0..=1.0).size(L).tip("Gain")`, no `.el`/`.el()`. Additive: `.el.size(L)` still compiles, so no rule; the repo's uses were rewritten by hand |
| `Ui` ids | every `Ui` method naming a node (`state`, `tween`, `tween_with`, `play`, `replay`, `memo`, `edit`, `edits_for`, `get`, `set_text`, `focus`, `focused`, `keys`, `text`, `local`, `tag`, `scroll`, `double_click`, `start_drag`, `dropped_on`, `set_ime_caret`, `set_weld_*`) takes `impl Into<Id>`; `clicked_outside`/`dismissed` keep `&[&str]` | principle 6; widening, no rule |
| `Ui::new(Theme::DEFAULT)` | `Ui::default()` (rule); `Ui::new(theme)` stays for any other theme | a plugin no longer names `Theme`, so truce's prelude `Theme` stops clashing with ours |
| `.font(f)` / `set_font(Option<Font>)` | both stay | the builder sets up a `Ui`; `set_font` swaps or clears it on a live one (a settings change), which a by-value builder cannot do |
| `MuiEditor::new(.., (u32, u32), ..)`, `.resizable((u32, u32))` | `impl Into<Size>`; `Size` converts from any pair of numbers | `(300, 200)` still reads the same |
| `bridge.meter(bridge.params().level.id())` | `bridge.meter(P::Level)` (the derived id enum already names meters) and `meter(ui, id, level)`, a preset drawing the standard level bar on a spring | the gain plugin's hand-built bar was the pattern every plugin copies |
| truce's `Theme` vs ours | nothing renamed: with `Ui::default()` the gain plugin names neither; if a plugin needs ours, `mui::prelude::Theme` | both preludes are globs; the explicit path is the documented pattern (a comment in the gain plugin) |
| `icon(font, sym)` | `icon(sym)`, drawn in `Theme::icon_font` (an `Option<Font>`, `None` by default); `.font(f)` overrides it. Rule: `icon(f, s)` -> `icon(s).font(f)`, with a note to move a shared face into the theme | one argument per icon; the icon face is set once. An icon is marked by the `Element::ICON` flag, read at resolve time |
| `Theme: Copy` | `Theme: Clone` (it holds the icon `Font`, an `Arc`) | `ui.theme().clone()` where a copy was taken; the resolver borrows it |
| playground `border(c, w)`, `width`, `height`, `pad_xy`, `no_border` | `stroke(c).stroke_width(w)`, `w`, `h`, `pad(x, y)`, `no_stroke` | the text DSL follows the Rust names |
| scrollbar heat tween key `format!("/bar{key}")` | `Id::runtime("/bar").field(key)`, `/bar/<key>`, built in place (inline for short keys) | no `String` per bar per frame |
| a runtime key read through `Ui` (`ui.scroll("/0")`) | `ui.scroll(Id::runtime("/0"))` | a `&str` converts through `Id::of`, which refuses `/` names in debug builds (principle 6) |
| `mui_symbols::sym` | in the prelude as `sym` (mui-scene depends on mui-symbols) | `icon(sym::HOME)` with no extra import |
| token pairs for `pad` | not supported: `(x, y)` is pixels; `.pad(M)` scales all four sides | the scaled padding is one `Spacing`; a per-axis token pair would need a second slot on `Node` for no caller yet |
