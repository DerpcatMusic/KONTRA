//! The design tokens: every length SMOOTH uses is stated here, once.
//!
//! Three rules hold the layer together:
//!
//! - A view states no length. It composes parts and asks for a group, and the
//!   group spaces itself.
//! - A part states no length either. It reads a rung off a scale below.
//! - A rung is *chosen*, not measured. Rhythm comes from [`space`], type from
//!   [`ty`], marks from [`stroke`], and a control's own dimensions from
//!   [`metric`].
//!
//! The rhythm is a 2-based ladder with a single-pixel rung for rules. Values
//! that fall between rungs snap to the nearer one: the design reads as a
//! system, which is worth a pixel.
//!
//! [`plot`] is the one exception. It holds soothe3's own axis as read off the
//! reference's pixels -- which frequency sits at which x, where zero sits --
//! and that is data about the engine, not a style decision.

// ------------------------------------------------------------------ rhythm --
/// The spacing ladder. Every gap and every pad is one of these.
pub mod space {
    /// Nothing at all.
    pub const NONE: f64 = 0.0;
    /// A hairline: one rule, one seam.
    pub const HAIR: f64 = 1.0;
    /// Inside a control, between things that are one thing.
    pub const TIGHT: f64 = 2.0;
    /// Between a mark and the name beside it.
    pub const XS: f64 = 4.0;
    /// Between a name and the value it names.
    pub const S: f64 = 6.0;
    /// Between siblings in a group.
    pub const M: f64 = 8.0;
    /// Between groups.
    pub const L: f64 = 12.0;
    /// A group set apart: the column's own margin.
    pub const XL: f64 = 16.0;
}

// -------------------------------------------------------------------- type --
/// The type ladder, and the three weights it is set in.
pub mod ty {
    /// A tick on an axis.
    pub const MICRO: f64 = 8.0;
    /// A tick that needs the half point: the frequency axis is dense.
    pub const TICK: f64 = 8.5;
    /// A name inside a small mark: the rail, a chip.
    pub const FINE: f64 = 9.0;
    /// A caption: a name under a dial, a label in the footer.
    pub const CAPTION: f64 = 9.5;
    /// A name that leads a line: the preset's title, a button's letter.
    pub const LABEL: f64 = 10.0;
    /// The reading size: a value, a field, a control's own name.
    pub const BODY: f64 = 11.0;
    /// A control's name at full size: the one letter button.
    pub const TITLE: f64 = 13.0;
    /// The wordmark, and nothing else.
    pub const WORDMARK: f64 = 28.0;

    /// A name is set between book and medium: the reference's labels are
    /// lighter than they first read at this size.
    pub const WEIGHT: f32 = 500.0;
    /// A value is set lighter than the name above it.
    pub const WEIGHT_VALUE: f32 = 340.0;
    /// The wordmark is the lightest thing in the editor, and the roundest.
    pub const WEIGHT_WORDMARK: f32 = 540.0;
    /// Only the wordmark reads this axis.
    pub const ROND: f32 = 100.0;
}

// ------------------------------------------------------------------ strokes --
/// Line weights. A mark is one of these, never a number at the call site.
pub mod stroke {
    /// A grid line, a focus ring: the thinnest thing drawn.
    pub const HAIR: f64 = 1.0;
    /// A circle's outline, a plot trace's rim.
    pub const RULE: f64 = 1.2;
    /// A drawn mark: an icon's stroke, a peak.
    pub const MARK: f64 = 1.4;
    /// The response curve itself.
    pub const CURVE: f64 = 2.0;
    /// The selected band's own curve.
    pub const SELECTED: f64 = 2.5;
}

// ------------------------------------------------------------------- radii --
/// Corner radii. A pill is always half its height, so only the corners the
/// design actually rounds are stated; everything else derives.
pub mod radius {
    /// The window's own corner.
    pub const WINDOW: f64 = 12.0;
    /// Anything that reads as a pill: a tab, a switch, a round button.
    pub const SELECTOR: f64 = 9.0;
    /// A field, a plate, a small box: barely rounded.
    pub const FIELD: f64 = 4.0;
    /// A card's corner.
    pub const BOX: f64 = 10.0;
    /// A well: the plot's inner corner.
    pub const CONCAVE: f64 = 8.0;
}

// ------------------------------------------------------------------ control --
/// A control's own dimensions. These are what a control *is*, so they are
/// named here rather than derived from the rhythm.
pub mod metric {
    /// A round button in the toolbar.
    pub const BUTTON: f64 = 20.0;
    /// A round button that carries a word: the reference sets it one pixel
    /// smaller than its neighbours.
    pub const BUTTON_SMALL: f64 = 19.0;
    /// A pill's height: a switch, a tab, a value's plate.
    pub const PILL: f64 = 19.0;
    /// A pill inside a line of type.
    pub const BADGE_H: f64 = 14.0;
    /// A badge's width: its longest word set in a hairline box.
    pub const BADGE_W: f64 = 33.0;
    /// A value's plate.
    pub const FIELD_H: f64 = 19.0;
    /// A value's plate, wide enough for a signed reading: `0.0 dB`.
    pub const FIELD_W: f64 = 62.0;
    /// A dial's reading: wide enough for `10.0`, no wider.
    pub const READING_W: f64 = 34.0;
    /// The engine's quality list: wide enough for its longest entry and the
    /// chevron that opens it.
    pub const DROPDOWN_W: f64 = 86.0;
    /// The preset bar. Wide enough for the title, the pager and the tags.
    pub const PRESET_W: f64 = 333.0;
    /// The settings and help menus' width.
    pub const MENU_W: f64 = 230.0;
    pub const PRESET_H: f64 = 21.0;
    /// The two-way switch.
    pub const SWITCH_W: f64 = 68.0;
    pub const SWITCH_H: f64 = 21.0;
    /// The A/B pair and its arrow.
    pub const PAIR_W: f64 = 39.0;
    pub const COMPARE_W: f64 = 44.0;
    /// The chosen side's disc inside the A/B pill.
    pub const DOT: f64 = 15.0;
    /// A switch's hollow ring.
    pub const RING: f64 = 11.0;
    /// A dial. One number per size, so a knob is one call.
    pub const DIAL: f64 = 110.0;
    pub const DIAL_SMALL: f64 = 52.0;
    /// A dial's plate tucks into its halo by this much.
    pub const TUCK: f64 = 6.0;
    /// The rail's pitch between markers.
    pub const RAIL_PITCH: f64 = 134.0;
    /// The marker on the rail.
    pub const RAIL_MARK: f64 = 8.0;
    /// The rail's handle at the top.
    pub const RAIL_GRIP_W: f64 = 5.0;
    pub const RAIL_GRIP_H: f64 = 16.0;
    /// A word turned on its side: one line tall, as wide as it is long.
    pub const RAIL_LABEL_H: f64 = 30.0;
    pub const RAIL_WORD_W: f64 = 36.0;
    /// The side panel the rail opens, and a row of it.
    /// The band bar's edge rail, in its band's colour: a thin bar, as tall
    /// as the controls it bounds.
    pub const EDGE_W: f64 = 3.0;
    pub const EDGE_H: f64 = 22.0;
    pub const SIDE_W: f64 = 176.0;
    /// A side-panel value: one plate, as wide as the panel's contents. A
    /// pair of them (the low and high tilt) shares the same width.
    pub const SIDE_FIELD_W: f64 = 104.0;
    pub const SIDE_LABEL_W: f64 = 60.0;
    pub const SLIDER_W: f64 = 54.0;
    pub const SLIDER_BAR: f64 = 2.0;
}

// -------------------------------------------------------------------- icons --
/// One rung per context, so a mark is never sized at the call site.
pub mod icon {
    /// A caret in a pager. The Phosphor outline fills only part of its box,
    /// so the caret is drawn a size up to read at the same weight.
    pub const CARET: f64 = 9.0;
    /// A mark inside a footer control or a chip.
    pub const SMALL: f64 = 9.0;
    /// A mark in a pill, or the padlock.
    pub const BASE: f64 = 11.0;
    /// A mark in the toolbar's larger button.
    pub const LARGE: f64 = 14.0;
}

// -------------------------------------------------------------------- plot --
/// soothe3's own axis, as the reference draws it.
///
/// This is the one place a position is stated outright, because it is not a
/// style: it is the measurement the engine's response is read against. The
/// labels are drawn, and the curve's handles placed, from these numbers, so
/// the canvas and the ticks cannot drift apart.
pub mod plot {
    use super::super::skin::{GRAPH, RIGHT};

    /// The plot's frame: the right column, beside the toolbar and footer.
    pub const W: f64 = RIGHT;
    pub const H: f64 = GRAPH;

    /// The span the white curve and the handles are drawn over, and where
    /// zero falls in it: the reference's own, 25.6 dB above the line and
    /// 28.5 below it across the plot's height.
    pub const SPAN_DB: f64 = 54.1;
    pub const TOP_DB: f64 = 25.6;
    /// The reduction curve's own span: 18 dB above zero, 20 below it.
    pub const REDUCTION_SPAN_DB: f64 = 38.0;
    /// A band's depth control is in steps of this many decibels on the curve.
    pub const BAND_DB_PER_UNIT: f64 = 1.8;
    /// Where a flat response sits, in the plot's own space.
    pub const ZERO: f64 = H * TOP_DB / SPAN_DB;
    /// The grid's top margin.
    pub const GRID_TOP: f64 = 16.0;
    /// How many pixels a decibel of the white curve is worth.
    pub const PIXELS_PER_DECIBEL: f64 = H / SPAN_DB;
    /// How many pixels a decibel of the reduction curve is worth.
    pub const REDUCTION_PIXELS_PER_DECIBEL: f64 = H / REDUCTION_SPAN_DB;

    /// The level grid: a line every this many pixels at the default range.
    pub const LEVEL_STEP: f64 = 65.5;

    /// The frequency grid: where each line sits, what it is worth, and what
    /// it is labelled.
    pub const FREQ: [(f64, f64, &str); 8] = [
        (49.0, 100.0, "100"),
        (110.0, 250.0, "250"),
        (173.0, 500.0, "500"),
        (247.0, 1000.0, "1k"),
        (328.0, 2000.0, "2k"),
        (413.0, 4000.0, "4k"),
        (501.0, 8000.0, "8k"),
        (590.0, 16000.0, "16k"),
    ];

    /// A handle: a tall pill on the low and high bands, a dot between them.
    pub const PILL_W: f64 = 10.5;
    pub const PILL_H: f64 = 25.0;
    pub const PILL_INNER_W: f64 = 7.5;
    pub const PILL_INNER_H: f64 = 22.0;
    pub const DOT_R: f64 = 7.0;
    pub const DOT_INNER_R: f64 = 4.6;

    /// The soft edge on the material's top: how far one band of it reaches,
    /// and how many bands it takes to fade into the background.
    pub const GLOW_STEP: f64 = 3.0;
    pub const GLOW_LAYERS: usize = 14;
    /// How many samples either side the halo's own edge is averaged over, so
    /// a step in the trace cannot stack its bands into a bright column.
    pub const GLOW_SOFTEN: usize = 24;
    /// How many samples either side the drawn trace is averaged over. The
    /// engine's per-bin reduction is jagged at the bin rate; the reference's
    /// trace is a smooth line, so the drawn edge takes a short average.
    pub const TRACE_SOFTEN: usize = 3;

    /// The well the plot sits in: its shadow's reach.
    pub const WELL_TOP: f64 = 9.0;
    pub const WELL_LEFT: f64 = 13.0;

    /// The axis labels: one cell per grid line.
    pub const TICK_CELL: f64 = 30.0;
    pub const TICK_ROW_H: f64 = 12.0;
    pub const LEVEL_CELL: f64 = 11.0;
    pub const LEVEL_COL_W: f64 = 24.0;
    /// The running range chip, in the plot's corner.
    pub const RANGE_W: f64 = 31.0;
    pub const RANGE_H: f64 = 15.0;
    /// Never draw a level label on the zero line: the handles rest there.
    pub const ZERO_MARGIN: f64 = 1.0;
}
