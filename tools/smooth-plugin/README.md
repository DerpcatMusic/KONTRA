# SMOOTH

A spectrally selective dynamics editor, and the plug-in that hosts it.

- `tools/smooth-ui` -- the interface: `skin` (tokens and parts), `view` (the
  tree), `graph` (the response plot). One library, two consumers.
- `tools/smooth-core` -- the engine. This is the Resonance Lab's clean-room
  reimplementation of soothe3's suppressor, ported from the reverse
  engineering in `Research/t3-research` (the canonical log and exponential
  programs, the projection, the spatial operators, the weighting banks, the
  reduction arithmetic). It is the research's code, kept whole so it can be
  diffed against the source it came from.
- `tools/smooth-preview` -- renders the interface to a PNG, for comparison
  against the reference: `cargo run -p smooth-preview --release -- out.png [scale]`.
- `tools/smooth-plugin` -- the plug-in: the parameters, the host plumbing, and
  the window the tree is hosted in.

## Parameters

The ranges, defaults and units are soothe3's own, read out of the binary's
declared parameter records in the reverse engineering: `Depth`, `Detail`,
`Attack` and `Release` run 0..10, `Mix` is 0..100 %, `Wet trim` and `Out trim`
are ±18 dB (the wet trim scales the processed signal before the mix, the out
trim after it), `Mode` is soft or hard, and the four bands carry a frequency,
a depth, a Q and a shape.

## Building and installing

```
cargo moose install --clap --vst3 -p smooth-plugin
```

Lands in `~/.clap/SMOOTH.clap` and `~/.vst3/SMOOTH.vst3`. Rescan the host's
plug-in list.

## Checking it

```
cargo test -p smooth-core -p smooth-plugin   # the engine's numerical contracts
cargo test -p smooth-ui                      # the interface's rules
python3 tools/smooth-plugin/scripts/clap_smoke.py ~/.clap/SMOOTH.clap
xvfb-run -a python3 tools/smooth-plugin/scripts/gui_shot.py ~/.clap/SMOOTH.clap shot.png
xvfb-run -a -s "-screen 0 1600x1000x24" python3 tools/smooth-plugin/scripts/gui_shot.py \
    ~/.clap/SMOOTH.clap shot.png --click 95,93 636,458
```

The smoke script is a small CLAP host: it loads the plug-in, turns the
knobs, and measures what comes out. The screenshot script opens the editor in
a real host window and photographs it; `--click` clicks points in the
editor's own 800x473 space and photographs each result, and `--drag` takes a
graph handle.

## The interface's rules

The editor is one intrinsic tree, and the rules that keep it one are checked
rather than remembered:

- `tokens` states every length: the spacing ladder, the type ladder, the
  strokes, the control metrics, the corners. A view composes parts; a part
  reads a rung. `tests/tokens.rs` fails if a layout call anywhere in `view`,
  `skin` or `graph` is given a bare number.
- A control owns its own metrics. `pill`, `field`, `plate`, `toggle_pill`,
  `icon_button`, `pair_pill`, `preset_bar`, `segmented` and `dropdown` take a
  value or a choice, never a width, and a pill's corner is always half its
  height.
- Marks come from the Phosphor pack (`icons.rs`, MIT): real path data, scaled
  by `glyph` from its own 256-unit space. The second test in `tests/tokens.rs`
  fails on path data typed into a view, so a hand-drawn mark cannot come back.
- The plot's axis (`tokens::plot`) is the one exception: those numbers are
  soothe3's own, read off the reference's pixels, and they are data about the
  engine rather than a style.
- The look is data. `smooth_ui::theme` holds a `Palette` per look -- every
  colour and every wash -- and the parts read `t.ink`, `t.field`, `t.veil`,
  never a hex literal. `voice` is the reference's own palette and the default;
  `console` and `paper` are the same instrument under different light. A third
  test in `tests/tokens.rs` fails on a colour stated outside `theme.rs`, and
  `voice` renders pixel-identically to the tree before the palette existed.
- A plate and the ink it carries are a pair: `selected` has `on_selected`,
  because a dark look wants the chosen segment to be the light one and its
  label to be dark.

### Choosing a look

```text
SMOOTH_THEME=console reaper          # a host reads the environment
cargo run -p smooth-preview --release -- out.png 1.5 paper
```

An unknown name is ignored rather than fatal, so a stale variable cannot stop
music.

## What is finished

- The engine meets the research's acceptance criterion, and the mix ramp is
  bit-identical to the reference's own gain helper, checked against the
  reference's own execution (see `tools/smooth-core/research/kernel/`).

- The dials are host parameters, and a drag on one is a gesture the host
  records.
- The graph's four handles drag, and they drive the four bands' frequency and
  depth. The axis is the reference's own, read off its frequency grid.
- The plot is the engine's own picture: the audio thread publishes the
  spectrum and the reduction, and the editor draws the reduction it is
  actually applying.
- The soft/hard switch, the bypass, the compare switch, the short-window
  switch and the quality dropdown all write their parameters.

## What is not finished

- The engine now meets the research's acceptance criterion: a hard-mode 4 kHz
  tone comes out 4.9 dB down. The criterion was written against 1 kHz, where
  the reference's own final weighting closes the bottom of the range, so the
  port follows the reference's shape rather than the letter of the old test.
  `tools/smooth-core/README.md` has the numbers and the remaining frontiers.
- A handle drag is written straight to the band parameter, so the host sees
  the value but does not record it as an automation gesture the way the knobs
  do.
