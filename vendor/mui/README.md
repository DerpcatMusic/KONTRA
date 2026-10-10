# MUI — a styled tree in, pixels and gestures out

**MUI** (Matari-UI) is the UI foundation for [Matari Audio](https://github.com/Matari-Audio)
plugins. You write a tree the way you would write CSS flexbox with tokens; MUI
lays it out intrinsically, turns every `.union` group into one filleted outline,
derives every shell as a true parallel inset of the outline before it, colours
every surface from a role palette, and hands a z-ordered paint list to Vello.
Layout is intrinsic: a float names a region around another node and
a slider thumb sits where two flex weights put it; explicit offsets are available when needed. **No runtime style
strings**: there is no `.class("btn btn-sm")` and there will not be one --
every value in the DSL is a Rust expression the compiler already checks.

Every crate is `#![forbid(unsafe_code)]` except `mui-baseview` and
`mui-truce`, which deny it and allow a few blocks: the wgpu surface on the
native window, `Send` for the window handle and the host's parent handle.
Every library crate compiles to `wasm32-unknown-unknown`; the native preview
host, `mui-baseview`'s window and the example plugin do not. The
geometry, layout, style and motion crates stay small -- `mui-motion` has no
dependencies at all -- but the renderer, text and accessibility stack does
not: Vello, wgpu, harfrust, accesskit and truce put about 440 packages in
`Cargo.lock`.

## Try it in your browser

[Open the MUI playground](https://matari-audio.github.io/MUI/) to edit welded
shapes, curved partitions, cutouts and layouts with immediate visual feedback.
It accepts a documented subset of Rust builder expressions and runs the actual
MUI scene engine with **Vello CPU compiled to WebAssembly** in a worker. It does
not compile arbitrary Rust or measure native GPU performance. Edits stay in your
browser; shared links carry the source in their URL fragment.

Run locally with `./playground/build.sh` (requires the WASM Rust target and
`wasm-bindgen-cli` 0.2.128), then serve `playground/` with any static web server.

## Under the hood

MUI owns layout, contours, input, animation and the paint list. Vello renders it:
`mui-vello` provides CPU rendering (`vello_cpu`) and `GpuRenderer`, a retained
renderer over classic Vello on wgpu 30. The host must keep the `GpuRenderer`
alive between frames to benefit from its caches. An unchanged
frame into the view they presented last records no GPU pass; a swapchain hands
out a new view per frame, so there the host saves the pass by not asking.

Plugin windows also have an automatic software fallback: Vello CPU rasterizes
into native pixel buffers without opening a wgpu device. `MUI_RENDERER=cpu`
forces that path for diagnostics. It retains unchanged frames and uses one CPU
thread; changed frames repaint fully. See the [plugin host contract](crates/mui-truce/README.md).

The classic-versus-Hybrid measurements that led here (history: MUI shipped on
`vello_hybrid` first) live in the
[rendering investigation](docs/rendering-investigation.md), including their
limits.

## Contours are layout too

Use `.union(fill)` for a shared outline, `.inside(padding)` to partition a parent's
contour, `.bend(amount)` for a curved divider, and `.cut(shape)` / `.keep(shape)`
for boolean regions. Borders and parallel shells follow the resulting outline.
`BorderRamp` gives a shared contour continuous color while identifying its tabs.
See the [shape-layout guide](docs/shape-layout.md) for the full contracts and
composable examples; the browser playground supports a smaller, explicit subset.

## One frame

```rust
use mui::prelude::*;

let mut ui = Ui::default();
let mut cutoff = 0.5;
let mut bypass = false;

// Built every frame, like an immediate-mode tree. Widgets read last frame's
// gesture on their id, so state lives in your own variables.
let bypassed = toggle(&mut ui, "bypass", "Bypass", &mut bypass).size(S).tip("Bypass the filter");
let root = col![
    row![title("Filter"), spacer(), bypassed].center(),
    slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0).value_text(format!("{:.0} Hz", cutoff * 20e3)),
    meter(&mut ui, "level", cutoff),
]
.gap(M)
.pad(L)
.radius(20)
.fill(Role::Surface)
.shadow(Shadow::soft(12.0));

let frame = ui
    .frame(root, Some(Size::new(280.0, 120.0)), Input::default(), 1.0 / 60.0)
    .unwrap();
assert!(frame.scene.paint.len() > 5);
// mui::vello::paint(&mut Gpu { scene, resources, cache, atlas: None }, frame.scene, Affine::IDENTITY)?;
```

`Ui::frame` takes an `Input` — pointer, wheel, key presses and typed text —
advances gestures and springs, styles the tree by state (hover and press are
mixed into a named node's fill through a spring), resolves layout and
geometry, and rebuilds the hit regions from the paths it painted, so what
responds and what you see cannot drift apart. A `PointerInput` converts into
an `Input`, so a pointer-only host passes one unchanged. What comes back:
`frame.animating` says whether to schedule another frame, `frame.cursor` is
what the hovered surface asks for, and `frame.tip` is the tooltip that came
due (already floated into the scene, handed back for a host that would
rather open a native window). Keys reach a widget through `ui.keys(id)`,
which is empty unless `id` is focused, and a global shortcut through
`ui.shortcuts()`, which is not gated by focus at all — except by the one
rule every editor has: a focused `text_input` consumes the stream, so a `z`
in a search box is a `z` and not an undo.

## The DSL

```rust
use mui::prelude::*;

let control = |id: &str| block(28.0, 28.0).pill().fill(Role::Primary).id(id).cursor(Cursor::Hand);

// A tab whose shell is a parallel inset of its own rounded outline.
let tab = col![control("plus"), control("phase"), control("warp")]
    .gap(10.0)
    .pad(22.0)
    .min_w(92.0)
    .center()
    .id("tab")
    .shell(12.0, Role::Raised);

// The tab and the panel unioned into one filleted shape.
let root = row![tab, block(520.0, 230.0).id("panel")]
    .start()
    .union(Role::Surface);

let scene = resolve(&SceneSpec::new(root)).unwrap();
assert_eq!(scene.surface("tab").unwrap().frame.size.width, 92.0);
```

| you write | it means |
|---|---|
| `row![..]`, `col![..]`, `stack![..]`, `grid![3; ..]` | flex axis, stack, cells; each arg goes through `IntoEl`, so `&str` is a text run |
| `row([..])`, `col([..])`, `stack([..])`, `grid(3, [..])` | the same four, taking an iterator |
| `block(w, h)`, `spacer()`, `text("..")` | a sized box, a `grow(1)` gap, a measured text run |
| `title("..")`, `body("..")`, `caption("..")` | text at the theme's `type_scale`: 18, 13 and 11 px by default |
| `icon(sym::HOME)` | a Material Symbols glyph in `Theme::icon_font`; `.font(f)` for another face |
| `.at(8, 4)`, `.offset(dx, dy)`, `.centered()`, `.centered_at(dx, dy)` | placement in a stack or grid cell: from the top-left, a nudge, or centred. Any number type works in every numeric argument |
| `.gap(M)`, `.pad(S)`, `.pad(12)`, `.pad((16, 8))`, `.gap(step(1.5))` | spacing tokens `Xs S M L Xl` from the theme, `n` units of its grid, or pixels; a pair is `(x, y)`, `Insets` sets each side |
| `.grow(w)`, `.shrink(w)`, `.basis(px)` | flexbox weights: `.grow(1)` takes a share of the surplus |
| `.w(Len::Pct(50))`, `.aspect(16.0 / 9.0)` | percentage and ratio sizes |
| `.w(clamp(64.0, 30.0, 220.0))` | CSS `clamp(min, pct%, max)`: fluid between two pixel stops |
| `.w(cq(40.0))` | a share of the nearest ancestor with a definite size on that axis -- CSS `cqw`/`cqh`, with no `container-type` to declare |
| `fits![wide, mid, thin]`, `fits([..])` | SwiftUI's `ViewThatFits`: the first candidate that measures inside the room on offer is the one that lays out and paints, no second build pass |
| `SceneSpec::new(root).scale(2.0)`, `Ui::scale` | the host's device pixels per unit: every edge and baseline the walk paints lands on the device grid, so abutting fills have no seam |
| `.w(120)`, `.h(40)`, `.square(28)` | the same sizes taking a bare integer |
| `.align(..)`, `.justify(..)`, `.anchor(x, y)`, `.offset(dx, dy)` | cross axis, main axis, stack placement, nudge |
| `.at(dx, dy)`, `.centered_at(dx, dy)` | placed from the parent's top-left corner, or from its centre |
| `.center()`, `.start()`, `.end()`, `.between()` | the four alignments worth a word |
| `.justify(Justify::SpaceAround)`, `.justify(Justify::SpaceEvenly)` | the other two CSS distributions |
| `.wrap()` | a row or column that breaks into lines instead of overflowing |
| `.wrap().line_gap(Xs)` | the space between wrapped lines (and between grid rows) apart from the `gap` between items along a line |
| `.span(2)`, `.order(-1)` | a grid cell two columns wide; placed before its declaration slot |
| `.min_col(120.0)` | `repeat(auto-fit, minmax(120px, 1fr))`: the grid drops columns until each clears 120 px, and a hugging grid widens to it rather than squeezing a column under it |
| `.push(child)`, `.baseline()`, `.lines(2)` | append to a container, sit text children on one baseline, cap a wrapped label |
| `.fill(Role::Primary)`, `.fill(Color::..)`, `.fill(Gradient::vertical(a, b))` | a palette role, a literal, a gradient |
| `Gradient::linear(180., ..)`, `::radial((0.3, 0.3), 0.6, ..)`, `::conic(-135., ..)` | the three ramps, stops as roles or colours: a knob arc is a conic gradient and no geometry |
| `.fill(Fill::Image(img, Fit::Cover))` | an RGBA buffer as a fill: `Cover`, `Contain` or `Fill` (`vello_cpu` paints the pixmap, `GpuRenderer` hands Vello the image once per buffer) |
| `.stroke(Role::Ink)`, `.radius(8.0)`, `.pill()`, `.shadow(Shadow::soft(12.0))` | outline, corners, a shadow appended to the list |
| `.radius(Corner::Field)` | the theme's radius for this kind of thing: `Selector` (toggle, badge), `Field` (button, input, tab), `Box` (card, panel) |
| `.corners(CornerStyle::Squircle)` | the curve the corners turn through, apart from how big they are: a continuous superellipse instead of a circular arc, through welds, shells and strokes alike |
| `.segmented()` | butt a row's or column's children into one strip: the gap closes, every seam goes square (children pushed later too), the container's own corner rounds the two ends |
| `.shadows([a, b])`, `.elevation(Elevation::Raised)` | replace the list; a contact and an ambient shadow, from the theme's steps |
| `.shadow(Shadow::inset(4.0))` | cast inward instead, clipped to the outline: a recess, a floor under glass |
| `.backdrop_blur(8.0)` | before the node paints, blur what is already painted behind it, clipped to its outline: a modal's frosted dim. Both renderers paint the scene under it once more through a real Gaussian; a canvas without filter layers shows the fill alone |
| `.stroke(Role::Ink.alpha(0.12))` | a role at an alpha: a hairline that still tracks the palette |
| `.preset(card())`, `.base(panel())` | merge a prepared `Style` over or under this one, field by field: the side that states something wins. Both are moved, not copied |
| `panel()`, `card()`, `glass()`, `chip("A")`, `tile(el)` | the presets in `mui::presets`: three styles to merge, two elements to finish. `glass()` is a translucent fill, a bright 1 px edge and an inner floor; add `.backdrop_blur(r)` to frost what is under it |
| `.apply(f)`, `.when(cond, f)` | hand the node to a builder run, conditionally or not |
| `.on(State::Hover, \|s\| s.stroke(Role::Ink))` | the look for a state, declared beside the resting one; `Hover`, `Press`, `Focus`, `Disabled` |
| `.disabled()`, `.when(bypassed, Styled::disabled)` | switch this node and its subtree off: the `State::Disabled` look, out of the hit map, out of Tab, and `disabled` to a screen reader. A gesture in flight on it is cancelled |
| `.full()` | all of the parent, both axes |
| `.animate()`, `.animate_with(Spring::new(0.3, 1.0))` | this node's fill, stroke, radius, text size and shadow spring to their new values |
| `.shell(d, fill)` | a parallel inset of the outline before it, cumulative |
| `.inside(2.)`, `.bend(0.2)` | [shape-aware layout](docs/shape-layout.md): nested regions inherit their parent's contour, with border-aware padding and normal-clearance split gaps |
| `.union(fill)` | paint the union of the children's outlines as one filleted vector shape; its shadow is the union of their blurs. `.weld(Weld::default())` / `weld![..]` blend the children's paint across the seam instead |
| `.cut(el)`, `.keep(el)` | boolean difference and intersection against a child placed like any floating one: a hole, or only the overlap. The shell, the stroke and the clip all follow the result, as they do a union. On a block, the block becomes a stack of its own size to hold the carve |
| `.mask(fill)` | paint `fill` source-atop the node's own subtree: a scroll fade is a ramp from transparent to the surface colour. It paints onto the shape, it cannot erase alpha -- an alpha mask layer is CPU-only in vello |
| `.blend(Mix::Multiply)`, `.opacity(0.5)` | composite this node's whole subtree as one layer |
| `.scroll()`, `.clip()`, `.float()` | overflow the wheel slides, overflow cut off, a child painted over everything |
| `.no_scrollbar()` | drop the overlay scrollbar a `.scroll()` node otherwise shows while it overflows: a thin `Ink` thumb over the far edge of the viewport (and the bottom one, for a row) that thickens under the pointer, drags, and jumps to a press on its track. The `Ui` paints and drags it; a scene resolved without one paints none |
| `.sticky()` | hold the leading edge of the enclosing `.scroll()` viewport -- the top of a column, the start of a row -- while this node's section (its own parent) is in view, then let the next section push it off. It keeps its slot in the flow and paints over the siblings that scroll under it, inside the same clip. `ui.min_size()` (and `Layout::min_size`) is the other half of a scrolling shell: the floor a plugin host refuses to resize below |
| `.pin(Pin::to("field").area(Area::Bottom).gap(Xs).match_width().fallback(Area::Top))` | a float placed against another node by name: one of nine named regions around it, a gap, a size taken from it, and areas tried in order until one fits the window. `.tip("..")` is this. Keep `.offset(dx, dy)` for the nudge no region can name |
| `canvas(\|size\| vec![Draw::fill(path, Role::Ink)])` | your own paths, in the node's own space |
| `Draw::fill(path, Role::Ink).tag("band")`, `Draw::hit(path, "knot-0")` | a drawn shape that is also the node's hit shape, by name, and hit geometry that paints nothing. Tag one draw and the node responds inside its tagged paths only: `ui.tag("dial")` says which, latched for the length of a drag |
| `.cursor(Cursor::Hand)`, `.tip("..")`, `.focusable()` | the pointer, a tooltip after half a second, Tab stops here |
| `.captures_wheel()` | a node that uses the wheel itself: over it, no enclosing `.scroll()` moves |
| `let Response { el, changed } = slider(&mut ui, "cut", "Cutoff", &mut hz, 20.0..=20e3)` | the one widget shape: `&mut Ui`, the id, the label (controls), what it edits by `&mut`, then its range or options; a `Response` out, the element and what happened last frame. `button` says whether it was clicked, `toggle`, `slider`, `knob`, `drag_value`, `text_input` and `color_picker` whether the value changed, `curve` and `bins` which part the gesture edited. A `Response` drops into `row![..]` whole |
| `button(&mut ui, "save", "Save").variant(Variant::Soft).size(S).tip("Save")` | a control's look and size: `Solid`, `Soft`, `Outline`, `Ghost`, and the same five sizes everywhere. `.role(Role::Danger)` recolours it, `.px(72)` is the hatch. The look goes on the response, then any element verb (`.tip`, `.w`, `.grow`, ..) finishes the control and keeps `.changed` |
| `slider(&mut ui, "cut", "Cutoff", &mut hz, 20.0..=20e3).value_text(format!("{hz:.0} Hz"))` | what the readout says, in the parameter's own units, instead of the default two decimals. The string is also what the readout is measured for, so nothing shuffles as digits come and go; a knob has no header, so it says this under the dial in place of its label |
| `text_edit(&mut ui, "notes", &mut s, TextOpts { newline: Newline::Enter, rows: 6, .. })` | a field of many lines: wraps to its width, Up/Down/Page move the caret by line, the lines scroll inside it. `Newline::ShiftEnter` keeps Enter for submit; `TextEdit { changed, submitted }` says what it did, `blur_on_submit` lets go of the focus |
| `drag_value(&mut ui, "bpm", "Tempo", &mut bpm, 20.0..=300.0)` | a number to drag sideways, 200 px across the range; a double click or Enter opens a field to type it, Escape cancels. A `Control`, so `.value_text(..)` applies |
| `color_picker(&mut ui, "tint", &mut color, ColorOpts { alpha: true })` | a saturation-value square, a hue strip, an alpha strip when asked, and a hex field |
| `stepped(&ui, id, &mut v, &range)` | the arrow, Page and Home/End keys of a slider, for a control of your own |
| `curve(&mut ui, "env", &mut env)` | an envelope over `mui::scene::curve::Curve`: the model's own cubics as one stroked path, a knot per point and two tension handles per segment, each its own hit shape. Edits `env` in place; `changed` is a `CurveEdit` saying what the drag moved -- Shift drags fine, Alt at the press locks an axis, `ui.tag("env")` names the shape under the pointer |
| `bins(&mut ui, "spectrum", &mut Bins { authored: &mut levels, .. })` | an additive spectrum: a bar per partial in the accent, the engine's live levels as a cap line over them, and a faint level grid. Edits the levels and the selection in place, as `curve` does; `changed` is a `BinEdit` saying what. One canvas and one hit shape -- pointer x becomes a bin index, so a drag paints every bin it crossed with no gaps, Shift refines from the press level, a secondary click resets one, and the arrows select and nudge. Over ~one bar a pixel the bins coalesce per column at their maximum, so 1024 partials still draw 200 bars. `bins_hover` is the index under the pointer, for a readout in your own units |
| `.a11y(A11y::Button)`, `.named("OK")` | what a screen reader hears: `mui-access` reads both off the surface. `.a11y(A11y::Image).named("Company logo")` is a picture with alt text; unlabelled, it is decoration |
| `.reserve("-88.8 dB")`, `ui.set_text("gain", v)` | measure a readout for the widest value it can show, then swap what it says without resolving the tree again: the frame stands, one glyph run re-shapes |
| `.text_weight(Weight::BOLD)` | the run's `wght` axis. A variable face moves; a static one has one weight and draws it |
| `Palette::from_seed(accent, Mode::Dark)` | a whole palette from one colour: brand roles around the seed's hue, greys tinted by it, signal hues left alone. Every role clears 3:1 on the background and the surface |
| `.id("name")` | a gesture target and a lookup key; unnamed nodes are decoration, unless they declare an `.on(State::Hover \| State::Press)` look |
| `.id(Id::of("osc").slot(3).field("gain"))` | the same key, composed: segments join with `/` and nothing reaches the heap under 46 bytes, so a rack of 374 named slots costs no `format!` per frame. `.id(..)` takes anything `Into<Id>` |
| `ui.start_drag(id, payload)`, `ui.dragging::<T>()`, `ui.dropped_on::<T>(id)` | drag-and-drop with a payload of your own type: attach it while the gesture drags, peek at it to light up a drop target, take it once when it lands. A drop elsewhere delivers nothing, and the ghost is yours -- a `.float()` pinned to the pointer's surface |
| `Path::from_svg_data("M0 0 h10 a5 5 0 0 1 0 10 z")` | an icon's `d` attribute as a `Path`, arcs and all |
| `ui.tween(id, target)`, `ui.edit(id)` | a spring-smoothed number; `Begin`/`End` of a gesture |
| `ui.get(id).mods`, `.press_mods`, `.button` | the modifiers now and at the press, and which of `Primary`/`Secondary`/`Middle` opened the gesture |
| `r.drag_fine(FINE_DRAG)`, `r.drag_axis()`, `r.clicked_with(Button::Secondary)` | Shift is the fine drag (`ui.drag` applies it already); the axis a drag has travelled furthest along; a click by one particular button |
| `r.activated()`, `r.double_clicked`, `r.wheel` | a primary click or Enter/Space on the focused target; the second press inside `ui.double_click` seconds; the wheel delta over the target this frame |
| `ui.dismissed(&["menu", "menu-button"])`, `ui.clicked_outside(..)` | a press outside every named node (and their descendants), or Escape: the close signal for a popup |
| `frame.clipboard`, `frame.edits` | what a copy wants put on the clipboard, and every gesture edge this frame |
| `Ui::new(theme).clipboard(board)`, `ui.set_clipboard(s)`, `ui.paste()` | a host's `mui::Clipboard` (`get`/`set`): text fields copy, cut and paste through it, and an app's Copy button sets it. With one, `frame.clipboard` stays `None` |

Alignment is inherited: a child without `.anchor` sits where its parent's
`align` and `justify` say, and `Stretch` is the default cross-axis value so
a row of controls fills its column unless told otherwise.

## A plugin editor shell

This is the shape of a synth plugin's editor. A header, a scrolling parameter
list, a response curve and a status bar are forty-six lines, twenty-one of
them the tree itself, and not one coordinate:

```rust
use mui::prelude::*;

let mut ui = Ui::default();
let (mut bypass, mut preset) = (false, "Init".to_owned());
let mut values = [0.4, 0.5, 0.8, 0.2, 0.6];
const NAMES: [&str; 5] = ["Drive", "Tilt", "Mix", "Air", "Floor"];

let params: Vec<El> = NAMES
    .iter()
    .zip(&mut values)
    .map(|(n, v)| slider(&mut ui, *n, n, v, 0.0..=1.0).el.into_el())
    .collect();

let curve = canvas(|size| {
    let pts = (0..=48).map(|i| {
        let t = f64::from(i) / 48.0;
        Point::new(t * size.width, size.height * (1.0 - t * t))
    });
    vec![Draw::stroke(Path::polyline(pts, false), Role::Primary, 2.0)]
});

let root = col![
    row![
        title("Synth"),
        text_input(&mut ui, "preset", &mut preset).el.w(140),
        spacer(),
        toggle(&mut ui, "bypass", "Bypass", &mut bypass).into_el().tip("Bypass"),
    ]
    .gap(S)
    .center(),
    row![
        col(params).gap(S).scroll().w(200),
        curve.grow(1.0).fill(Role::Raised).radius(12.0).cursor(Cursor::Crosshair),
    ]
    .gap(M)
    .grow(1.0),
    row![caption("48 kHz"), spacer(), caption("2 voices")].center(),
]
.gap(M)
.pad(L)
.fill(Role::Surface);

let frame = ui
    .frame(root, Some(Size::new(560.0, 340.0)), Input::default(), 1.0 / 60.0)
    .unwrap();
assert!(frame.scene.surface("Drive").is_some());
```

### The same tree at 240x600 and at 2000x300

A plugin window gets dragged to shapes nobody designed for. Reflow is three
declarations and no breakpoint: `clamp(min, pct, max)` is a length with two
stops, `.min_col(px)` makes a grid's declared column count a ceiling it drops
from, and `.wrap()` breaks a line.

```rust
use mui::prelude::*;

let editor = || {
    col![
        row![title("Synth"), spacer(), caption("v1.0")].baseline(),
        // Three tabs, fluid between 64 and 120 px, wrapping when they run out.
        row(["Osc", "Filter", "Env"].map(|n| {
            row![caption(n)].w(clamp(64.0, 18.0, 120.0)).pad((0.0, 8.0))
        }))
        .gap(S)
        .wrap(),
        // Four knobs, as many across as fit at 120 px a column.
        grid(4, ["a", "b", "c", "d"].map(|k| block(40.0, 40.0).id(k)))
            .gap(S)
            .min_col(120.0),
    ]
    .gap(M)
    .pad(M)
    .w(pct(100.0))
    .h(pct(100.0))
};

let stacked = |w: f64, h: f64| {
    let spec = SceneSpec::new(editor()).offered(Size::new(w, h));
    let scene = resolve(&spec).unwrap();
    let y = |k: &str| scene.surface(k).unwrap().frame.y;
    y("a") != y("b")
};
// One column in a thin window, four in a wide one. Same tree.
assert_eq!((stacked(240.0, 600.0), stacked(2000.0, 300.0)), (true, false));
```

The gallery's **Responsive editor** scene is this tree with knobs on it, and
`scenes::tests` resolves it at 240x600, 800x500 and 2000x300 asserting that
no child ever leaves its parent.

## Plugins

`mui-truce` puts a MUI tree in a CLAP or VST3 plugin built with
[truce](https://crates.io/crates/truce). `MuiEditor` is truce's `Editor`: a
baseview child window in the host's window, a wgpu surface on it, and one
`Ui::frame` per native event. `Bridge::bind` connects a widget id to a truce
parameter. A drag becomes the host's begin/perform/end, and host automation
is the value the next tree reads:

```rust,ignore
fn editor(params: Arc<GainParams>) -> Box<dyn Editor> {
    MuiEditor::new(params, Ui::default(), (300, 200), |ui, bridge| {
        let gain = bridge.bind(ui, P::Gain, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0));
        col![gain, title(bridge.text(P::Gain))].pad(L).fill(Role::Surface)
    })
    .resizable((260, 180))
    .into_editor()
}
```

`examples/gain-plugin` is a complete plugin: a gain knob, a bypass toggle and
an output meter. [crates/mui-truce/README.md](crates/mui-truce/README.md) has
the build commands and the clap-validator and pluginval results.

Build plugins with `--profile plugin` (release with `panic = "unwind"`), not
`--release`: release aborts on panic, so a bug in the editor would take the
DAW down instead of being caught at the FFI edge.

## Hosts

- `mui-baseview`: the native window, for apps (`run`) and for any plugin
  framework (`open`, parented under the host's window). A `View` trait, the
  event queue, the GPU surface, clipboard and AT-SPI.
  `cargo run --example app -p mui-baseview` is a knob in a window.
- `mui-truce`: truce's `Editor` over that window, plus the `Bridge` from widget
  ids to truce parameters.
- `mui-vello::host`: bring your own window and surface; `Host` presents a
  resolved scene to it. `Host::with_transparency(.., Transparency::Translucent)`
  composites with alpha where the surface offers it (`Host::translucent` says
  whether it did): paint a faded root, `Role::Surface.alpha(0.7)`, over an OS
  blur for frosted glass.

### Native graphics diagnostics and automatic MUI issues

Native hosts persist adapter attempts, startup stages, device errors and caught
window panics automatically. `mui::diagnostics::log_path()` finds the current
session; `MUI_DIAGNOSTICS_DIR` overrides its directory. Session files rotate at
1 MiB with one previous file, and startup retains the newest 64 session files.
Browser hosts use no-op native diagnostics.

Enable `mui`'s optional `reporting` feature, then retain one reporter in the
app/plugin's owned state **before opening windows**. Registration failure can
leave reporting disabled while the editor continues to work:

```rust,ignore
let mut config = mui::diagnostics::Config::new("my-plugin", env!("CARGO_PKG_VERSION"));
config.build = option_env!("APP_GIT_REVISION").unwrap_or("unknown").into();
config.mui_revision = "your-MUI-git-pin".into();
let reporter = mui::diagnostics::Reporter::start(config)
    .inspect_err(|error| eprintln!("MUI reporting unavailable: {error}"))
    .ok();
// Store `reporter` on owned app/plugin state until all MUI windows are closed.
```

The reporter queues bounded, sanitized diagnostics locally and delivers them
on a managed background thread to `https://matari-audio.com/api/support/mui`.
Failed submissions retain their original bytes and retry after a minute or on
the next load. A report is deleted only after support acknowledges its exact
SHA-256 and a verified issue URL in `Matari-Audio/MUI`. Apps contain no GitHub
credentials. The server groups matching failures across apps, publishes only
bounded environment metadata, and retains detailed evidence privately for
30 days. The endpoint must be deployed and configured before delivery works.
Set `MUI_REPORTING_DISABLED=1` in CI or an offline diagnostic session to retain
local evidence while preventing network delivery and live issue creation.

Startup, surface configuration and the first draw also use locked operation
markers. Normal completion and Rust unwind clear them; abrupt termination
leaves an **unconfirmed operation interruption** for recovery on the next
reporter load. Live operations in another process are skipped. This is not a
native stack dump and does not prove that MUI or a particular GPU caused the
host to terminate. Steady-frame native crashes and unrelated DAW crashes still
need OS postmortem evidence; no process-wide signal or panic hook is installed.
Custom hosts can use `mui::diagnostics::operation(..)` around their own risky
MUI startup calls and `error(..)` for MUI-owned failures.

Start and drop the reporter off the audio thread. Close/drop all windows first,
then drop the last reporter (put its field after window fields if relying on
struct field drop order). Shutdown joins its worker. HTTP has a five-second
deadline; the OS DNS resolver can take longer, so reporting shutdown can wait
for it. No timeout helper thread is detached from a plugin being unloaded.
The default queue holds 32 reports and 32 live or
interrupted operation markers per app directory. Export local diagnostics if
these limits are reached. Multiple instances of the same app/build share the
worker within one loaded MUI image.

## Motion

Nothing is keyframed. `.animate()` puts a spring on a node's own visual
channels -- fill (in Oklch, hue the short way round), stroke width, radius,
text size, shadow blur, shell depths -- and a new declared value **retargets**
the live spring instead of restarting it, so a colour changed mid-flight
keeps its velocity. `.animate_with(s)` is the same with your own spring;
`Spring::new(response_s, damping)` is the tuning you want (`1.0` is critical,
below that overshoots), `Spring::instant()` turns one off.

`ui.tween(id, target)` is the escape hatch for a number MUI cannot see --
a canvas sweep, a pan -- and `ui.tween_with` takes a spring. `frame.animating`
is true while any of them still moves, which is the only thing a host needs
to decide whether to schedule another frame.

Gestures have edges: `ui.edit(id)` reports `Edit::Begin` when a press
captures a target and `Edit::End` when it lets go (including a cancelled
one), which is exactly a plugin parameter's begin/end-edit bracket.
`frame.edits` is the whole list.

```rust
use mui::prelude::*;

let mut ui = Ui::default();
let mut gain = 0.5;
let sweep = ui.tween("sweep", gain);
let root = col![
    block(60.0, 60.0).fill(Role::Primary).animate().id("lamp"),
    knob(&mut ui, "gain", "Gain", &mut gain, 0.0..=1.0),
];
let frame = ui.frame(root, Some(Size::new(200.0, 200.0)), Input::default(), 1.0 / 60.0).unwrap();
assert!(sweep <= gain && frame.edits.is_empty());
```

## Images and icons

`Image::rgba(w, h, bytes)` takes straight (non-premultiplied) RGBA8 -- what a
decoder hands back -- and `Fill::Image(img, fit)` paints it into whatever
outline the node already has, cropped (`Cover`), letterboxed (`Contain`) or
stretched (`Fill`). Decoding is the host's job: no library crate takes an
image dependency. `Path::from_svg_data` turns an icon's `d` attribute into a
`Path` (arcs included), so a symbol is geometry like everything else.

`vello_cpu` paints the pixmap itself. `GpuRenderer` converts each image
buffer once into a Vello image that Vello uploads and samples; a GPU texture
registered with `GpuRenderer::set_texture` is sampled in place. What was uploaded (or premultiplied, on the CPU) lives in a
`mui_vello::Cache` owned beside the renderer. It holds only a `Weak` to each
buffer and sweeps the entries the app has dropped on the next image lookup,
so a panel handing over a fresh frame buffer every frame does not grow it.

## Accessibility

`mui-access` turns a `ResolvedScene` into an `accesskit::TreeUpdate`:
`tree_update(&scene, focus, scale)`, where `scale` is the window's device
pixels per scene unit. A node says what it is in the tree itself --
`.a11y(A11y::Button).named("OK")` -- and the walk carries that onto the
surface; a node with no role reports as a group, and a control with no
label is named by its id (a group with none stays unnamed). A `text_input` reports its line
as a `TextRun` with each character's position and its selection, so a reader
follows the caret, and a reader's `SetTextSelection` comes back as
`SemanticAction::set_selection`. The
widgets in `mui` already describe themselves, so the preview just hands the
update to its `accesskit_winit::Adapter` after each frame.

## Colour

A theme carries a `Palette`: a `Mode`, seven `Pigment`s (hue and chroma, no
lightness), and two steps. Every surface, every ink, every state and the
other theme are derived. Ink is not a colour you pick: `Role::Ink` and
`Role::Dim` resolve against the fill they sit on, and `Palette::on` is
checked at 4.5:1.

```rust
use mui::prelude::*;

pub const SKIN: Theme = Theme {
    palette: Palette {
        neutral: Pigment::new(264.0, 0.015),
        primary: Pigment::new(242.0, 0.131),
        step: 0.045,
        hover: 0.11,
        ..Palette::NEUTRAL
    },
    corners: Corners { box_: 28.0, concave: 32.0, ..Corners::DEFAULT },
    ..Theme::DEFAULT
};
let light = SKIN.palette.with_mode(Mode::Light);
assert!(light.is_valid());
```

`crates/mui-preview/src/skin.rs` is exactly this file. There is no
light-theme half because there is nothing in it a mode could contradict.

## Crates

[ARCHITECTURE.md](ARCHITECTURE.md) has the crate graph and the scene walk --
how welds, shells, carves and text become one paint list -- and
[docs/shape-layout.md](docs/shape-layout.md) the contour-aware layout
contracts.

## Preview

```bash
cargo run -p mui-preview
```

The whole window is one tree: the sidebar's scene list, toggles and sliders
are `mui` widgets, the specimen is an anchored child of the stage, and a drag
on the stage pans it through an offset. Text reaches the pixels as a hinted glyph
run, not a filled outline: the host hands `paint` a `mui_vello::Gpu`.
`bacon` rebuilds and relaunches on save. Point `MUI_PREVIEW_FONT` at a
variable font and the Glyph scene grows a slider per axis; point
`MUI_PREVIEW_THEME` at a `key = value` file and the palette reloads while the
window is open. F12 outlines every surface and names the one under the
pointer, and the title bar reads the resolve and paint cost of the last
thirty frames.

winit's wheel, keys and modifiers ride into `Input` with the last pointer
sample of each batch, so `Ui::scroll`, `Ui::focus` and `text_input` work in
the window: the Scroll, Text, Tooltip, Canvas, Drag, Image, Wrap, Motion,
Grid, Select and Responsive editor scenes are there to prove it -- one per
thing the library claims to do. Drag the window narrow and the last one
reflows: it is `pct(100)` of the stage, so it is measured against whatever
the host gives it. `Frame.cursor` is applied with `window.set_cursor`; `Frame.tip` is
not, because the tooltip is already floated into the scene. Printable
characters go to `Input.text` only -- `Key::Char` is emitted just for
ctrl/cmd shortcuts, or `text_input` would insert every character twice.
Copy, cut and paste leave and re-enter through `Frame::clipboard` and
`Input::clipboard`; the preview loops them back to itself, so it is its own
clipboard and never touches the OS one. An input method reaches a field
through `Input::ime`: a `Preedit` is painted under the caret and never joins
the value, a `Commit` inserts like typed text, and `Frame::ime` tells the host
where to put the candidate window.

```bash
cargo run -p mui-vello --example headless -- /tmp/pill.png
```

is the stack end to end with no window, and

```bash
cargo run -p mui --features cpu --example snapshot -- /tmp/widgets.png
```

runs two `Ui::frame`s of the widget card and rasterises them on the CPU:
no GPU, no window. With `--features cpu`, `mui-vello` renders through
`vello_cpu` and its snapshot test asserts actual pixels. `--features
cpu-threads` rasterises on a rayon pool instead of one core; it is off by
default because a snapshot-sized pixmap loses more to thread hand-off than
it gains (BENCHMARKS.md measures both).

```bash
cargo run -p mui-scene --profile perf --example stress
```

resolves a ~1000-node tree at three window shapes and counts the allocations
each resolve makes. It lives in `examples` because it installs a counting
global allocator, which the library itself forbids.

## Verify

### MUI tester and UI evidence

`cargo run -p mui-preview -- --test-ui` runs the existing gallery as a small
MUI tester. It traverses every registered scene at three window sizes in both
themes, sends pointer, drag, wheel, keyboard and text input, and exercises the
overlay path. New gallery scenes join the traversal automatically. Each case
requires a submitted native frame; completion is recorded after window teardown.
This covers the gallery and existing native regressions, rather than every MUI
API or every DAW host.

[`.github/workflows/ui.yml`](.github/workflows/ui.yml) runs on every Verify
event, independently of the library test jobs. Linux Vulkan/lavapipe and
OpenGL/llvmpipe presentation are required checks. Xvfb provides a real X11
display: the collector requires a nonuniform specimen in every native window
screenshot. Separate regressions check embedded editor pixels, expose, resize,
multiple windows, close/reopen, device loss and CPU fallback without a GPU.

Windows DX12 and macOS Metal are **informational native probes** on standard
VMs. An unavailable adapter or native fault fails the probe and saves evidence;
it is not converted into a skipped graphics test. They do not block the Linux
gate until the hosted adapter behavior is established. Neither those probes
nor software Linux rendering certify consumer NVIDIA/Intel driver support.

`tools/ci/ui.py --output ui-results --gallery -- COMMAND` supervises the tester
as a child process. On Linux, add `--record` and run inside a 1600×1200 Xvfb
display. Artifacts include adapter inventory, durable JSONL stages, stdout,
stderr, exit code/signal, screenshots, a video and per-scene cost summaries.
Measurements cover CPU resolve/submission time and retained texture memory;
they do not measure GPU completion or display latency. Interrupted journals
retain their last complete stage. CI disables network reporting to keep test
failures from creating production issues.

The [GPUI comparison](research/gpui-graphics-comparison.md) explains the device
requirements and recovery differences observed in upstream source.

### Plugin editor CI on GitHub runners

[`.github/workflows/plugin-ui.yml`](.github/workflows/plugin-ui.yml) is a reusable
workflow for already-built Linux x86_64 VST3 plugins. It runs pluginval 1.0.4
under Xvfb on Mesa software Vulkan and GL, with GUI tests enabled. Strictness 6
and three repeats exercise editor creation, close/reopen, automation and opening
while processing audio. Every job saves validator output, graphics environment,
MUI journals and a display recording, including on failure. A missing editor
test fails the job. CI disables automatic MUI issue submission.

Upload an **unpacked** bundle in the caller's build job, keeping the `.vst3`
directory in the artifact. Use a distinct artifact name for each plugin / call:

```yaml
# In the Linux build job, after producing target/bundled/MyPlugin.vst3:
- uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9
  with:
    name: my-plugin-linux-vst3
    path: target/bundled/

# Under the caller workflow's jobs:, alongside its build job:
ui:
  needs: build-linux
  uses: Matari-Audio/MUI/.github/workflows/plugin-ui.yml@FULL_MUI_COMMIT
  with:
    artifact-name: my-plugin-linux-vst3
    plugin-path: MyPlugin.vst3
    require-mui-diagnostics: true
```

Replace `FULL_MUI_COMMIT` with the reviewed workflow commit. The caller's build
must also pin instrumented MUI for `require-mui-diagnostics: true`; that option
requires a first-frame submission record and rejects MUI error records. It is
false by default for older plugin builds. The record proves submission to the
surface, not visible pixels. Pluginval does not assert screenshots or editor
resizing; MUI's native presentation tests assert displayed pixels, resize and
close/reopen separately, on both software backends.

[Standard runners are free for public repositories](https://docs.github.com/en/actions/concepts/billing-and-usage).
Software graphics catch lifecycle / rendering regressions but cannot reproduce
NVIDIA or Intel driver bugs. [GitHub's paid larger runners](https://docs.github.com/en/actions/reference/runners/larger-runners)
offer NVIDIA Tesla T4 on Linux / Windows and M2 GPU acceleration on macOS.
Use dedicated hardware runners for affected Intel / consumer NVIDIA models and
DAW-specific tests; the documented hosted GPU list does not offer Intel GPUs.
This workflow covers VST3: CLAP requires a host that exercises its GUI extension.
MUI's existing Windows verification deliberately skips GPU tests due to a WARP
fault, so a green Windows check currently does not certify Windows rendering.

### Library checks

```bash
./tools/verify.sh
```

Formatting, tests, clippy with warnings denied, and a wasm check of the
library crates. `BENCHMARKS.md` is the frame budget of a synth-sized scene
on `vello_cpu` and `GpuRenderer`, reproduced by
`cargo run -p mui-vello --profile perf --features cpu,gpu-effects --example bench`.
