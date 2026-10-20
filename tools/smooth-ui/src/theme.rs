//! What SMOOTH is made of, as a value.
//!
//! The lengths live in [`crate::tokens`]; the colours live here, in a
//! [`Palette`]. A part asks for a token by name -- `t.ink`, `t.field`,
//! `t.veil` -- and never for a hex literal, so a whole look is a data
//! structure rather than a fact about the source.
//!
//! Three palettes ship. [`VOICE`] is the reference's own, byte for byte: it is
//! the default and the one the parity measurements are taken against.
//! [`CONSOLE`] is the same instrument under stage light, and [`PAPER`] is the
//! same instrument on a bright desk. They are not recolourings of one accent;
//! each states its own ground, its own three weights of ink, its own plate,
//! its own washes, and its own plot, because a dark ground needs a different
//! separation between a plate and the surface it sits on.
//!
//! The active palette is thread-local, because an editor renders on one
//! thread. [`set`] picks one; [`t`] reads it. A preview passes
//! `--theme console`, a plug-in host reads `SMOOTH_THEME`, and a test can
//! render every palette to prove none of them fall back to a literal.

use std::cell::Cell;

/// A named look: every colour the editor draws, and the alphas it draws them
/// at.
///
/// The alpha fields are part of the theme rather than the code because they
/// are how a palette keeps its depth: a wash that reads as 24 % white on a
/// light ground reads as nothing on a dark one, so a dark theme lowers it and
/// raises its ink instead.
pub struct Palette {
    /// The window.
    pub ground: u32,
    /// What surrounds it: the desktop a preview is flattened onto.
    pub page: u32,
    /// The toolbar, one step lighter than the ground.
    pub bar: u32,
    /// The favourite mark.
    pub heart: u32,

    /// The control column, top and bottom of its gradient.
    pub panel_top: u32,
    pub panel_bottom: u32,
    /// The rail, left and right of its gradient.
    pub rail_left: u32,
    pub rail_right: u32,
    /// Edges: the window's rim, and the light that catches it.
    pub edge: u32,
    pub edge_light: u32,

    /// Ink: the reading colour, the quiet one, the wordmark, its accent, and
    /// the colour of text on a dark plate.
    pub ink: u32,
    pub ink_quiet: u32,
    pub ink_logo: u32,
    pub logo_accent: u32,
    pub mark: u32,

    /// Plates: a field, a chip, a chip one step lighter, a switch's track, the
    /// rail's thumb, the knob's body, that body lit, the chosen segment, and
    /// the two sides of a knob's ring.
    pub field: u32,
    pub chip: u32,
    pub chip_light: u32,
    pub toggle: u32,
    pub thumb: u32,
    pub body: u32,
    pub body_hi: u32,
    pub selected: u32,
    /// The ink on a [`selected`](Self::selected) plate. It is a token of its
    /// own because a dark ground wants the chosen segment to be the *light*
    /// one, and then its label has to be dark -- a plate and the ink it
    /// carries are a pair, and a theme states both.
    pub on_selected: u32,
    pub ring_dark: u32,
    pub ring_light: u32,

    /// The plot: the well it sits in, the grid, the grid's base line, and the
    /// fill under the curve.
    pub graph_top: u32,
    pub graph_bottom: u32,
    pub grid: u32,
    pub grid_base: u32,
    pub fill_top: u32,
    pub fill_bottom: u32,
    /// The plot's ink: its grid, its zero line, the response curve, and the
    /// rim of the reduction trace. It is separate from [`mark`](Self::mark)
    /// because a bright ground needs the plot drawn *darker* than itself
    /// while the marks stay light for the plates they sit on.
    pub plot_ink: u32,

    /// The four bands, in the reference's order: red, magenta, cyan, green.
    pub red: u32,
    pub magenta: u32,
    pub cyan: u32,
    pub green: u32,

    /// The base a wash is mixed from: white on a light ground, often a lifted
    /// ink on a dark one.
    pub plate: u32,
    /// The shadow a raised thing casts, the inset shadow of the plot's well,
    /// the rim of a handle, and the running range's plate.
    pub shade: u32,
    pub well: u32,
    pub handle: u32,
    pub range_chip: u32,

    /// How much of [`plate`](Self::plate) a raised control carries.
    pub wash: f32,
    /// The same, for a control that is already on a plate.
    pub wash_deep: f32,
    /// A grid line over the plot, and the zero line that is brighter.
    pub veil: f32,
    pub veil_strong: f32,
    /// The reduction trace's rim: softer than the curve itself.
    pub veil_soft: f32,
    /// The knob's and the plot's shadow.
    pub shade_alpha: f32,
    /// The well's two shadows, and the light line along its top.
    pub well_alpha: f32,
    pub well_edge: f32,
    /// A handle's rim, and its plate's opacity.
    pub rim: f32,
    pub chip_alpha: f32,
    /// The hairline that catches the plot's edge.
    pub edge_veil: f32,
    /// One band of the soft edge where the plot's material meets its
    /// background; the bands stack, so this is a fraction of the whole.
    pub glow: f32,
}

/// The reference's own look: soothe3's cool blue, read off its pixels.
pub const VOICE: Palette = Palette {
    ground: 0x97B1D2,
    page: 0xA3BCDB,
    bar: 0x9EB6D6,
    heart: 0xD2507C,

    panel_top: 0x9FB8D7,
    panel_bottom: 0x98B2D3,
    rail_left: 0x8DA3C2,
    rail_right: 0xA1B9DA,
    edge: 0x7088A9,
    edge_light: 0xCBD9EA,

    ink: 0x465773,
    ink_quiet: 0x5F6E85,
    ink_logo: 0x55669C,
    logo_accent: 0xC8DCF2,
    mark: 0xF2F4FA,

    field: 0xC1D1E4,
    chip: 0xC2D0E5,
    chip_light: 0xC9D7EA,
    toggle: 0xC3CFE5,
    thumb: 0x8199C4,
    body: 0x45454F,
    body_hi: 0x51515D,
    selected: 0x4C4C57,
    on_selected: 0xF2F4FA,
    ring_dark: 0x7188B2,
    ring_light: 0x9AB3D2,

    graph_top: 0xB0C4DD,
    graph_bottom: 0xBACDE6,
    grid: 0xB6CAE3,
    grid_base: 0xC6D8F0,
    fill_top: 0x8298CA,
    fill_bottom: 0x899ED2,
    plot_ink: 0xF2F4FA,

    red: 0xC46678,
    magenta: 0xAA61CA,
    cyan: 0x5293B1,
    green: 0x3E9E83,

    plate: 0xFFFFFF,
    shade: 0x33415E,
    well: 0x5C7398,
    handle: 0xF2F3F7,
    range_chip: 0xC7D7E9,

    wash: 0.24,
    wash_deep: 0.16,
    veil: 0.09,
    veil_strong: 0.20,
    veil_soft: 0.30,
    shade_alpha: 0.17,
    well_alpha: 0.24,
    well_edge: 0.20,
    rim: 0.96,
    chip_alpha: 0.90,
    edge_veil: 0.70,
    glow: 0.015,
};

/// Stage light: a deep slate ground, cool ink turned pale, and the plot
/// carried by its own glow rather than by a fill.
pub const CONSOLE: Palette = Palette {
    ground: 0x14171C,
    page: 0x0A0C0F,
    bar: 0x1B1F26,
    heart: 0xE06A8E,

    panel_top: 0x1A1E25,
    panel_bottom: 0x15181E,
    rail_left: 0x0E1014,
    rail_right: 0x1D222A,
    edge: 0x05070A,
    edge_light: 0x3A455C,
    ink: 0xC9D4E4,
    ink_quiet: 0x8E9DB4,
    ink_logo: 0x7FA6E8,
    logo_accent: 0x2A3646,
    mark: 0xF4F7FC,

    field: 0x232A34,
    chip: 0x20262F,
    chip_light: 0x262D38,
    toggle: 0x1D232B,
    thumb: 0x4A5568,
    body: 0x2A2F3A,
    body_hi: 0x333947,
    selected: 0xE8EEF8,
    on_selected: 0x14171C,
    ring_dark: 0x3C4553,
    ring_light: 0x6C7B92,

    graph_top: 0x191E26,
    graph_bottom: 0x141920,
    grid: 0x2A3341,
    grid_base: 0x39445A,
    fill_top: 0x27334E,
    fill_bottom: 0x202B43,
    plot_ink: 0xE6EDF9,

    red: 0xE4738A,
    magenta: 0xC079E0,
    cyan: 0x5FB3D6,
    green: 0x4FBFA0,

    plate: 0xFFFFFF,
    shade: 0x000000,
    well: 0x000000,
    handle: 0x1A1F27,
    range_chip: 0x232B36,

    wash: 0.10,
    wash_deep: 0.07,
    veil: 0.08,
    veil_strong: 0.18,
    veil_soft: 0.26,
    shade_alpha: 0.45,
    well_alpha: 0.55,
    well_edge: 0.40,
    rim: 0.95,
    chip_alpha: 0.85,
    edge_veil: 0.35,
    glow: 0.016,
};

/// Morning paper: a warm, near-white ground with ink that is almost brown.
/// The bands keep the reference's hues, because they are the bands.
pub const PAPER: Palette = Palette {
    ground: 0xF2EDE4,
    page: 0xDCD3C4,
    bar: 0xF7F3EC,
    heart: 0xC25064,

    panel_top: 0xF6F1E8,
    panel_bottom: 0xEDE6DA,
    rail_left: 0xD9D0C2,
    rail_right: 0xEFE9DE,
    edge: 0xB5A995,
    edge_light: 0xFFFDF8,

    ink: 0x4A4238,
    ink_quiet: 0x7A7062,
    ink_logo: 0x8A5A3C,
    logo_accent: 0xE8D9C6,
    mark: 0xFFFCF6,

    field: 0xE6DCCB,
    chip: 0xEDE4D4,
    chip_light: 0xF1E9DB,
    toggle: 0xE2D8C6,
    thumb: 0xB3A48C,
    body: 0x3E382F,
    body_hi: 0x4A4438,
    selected: 0x4A4238,
    on_selected: 0xFBF7F0,
    ring_dark: 0xC0B29B,
    ring_light: 0xE4DAC8,

    graph_top: 0xFBF7F0,
    graph_bottom: 0xF3EDE3,
    grid: 0xDCD2C1,
    grid_base: 0xCEC2AE,
    fill_top: 0xC9BDA8,
    fill_bottom: 0xD7CBB8,
    plot_ink: 0x6B5B45,

    red: 0xB9556A,
    magenta: 0x9550B0,
    cyan: 0x3D7E99,
    green: 0x34856D,

    plate: 0xFFFFFF,
    shade: 0x6B5B45,
    well: 0x8A7A62,
    handle: 0xFFFDF8,
    range_chip: 0xE9E0D1,

    wash: 0.42,
    wash_deep: 0.28,
    veil: 0.16,
    veil_strong: 0.30,
    veil_soft: 0.40,
    shade_alpha: 0.14,
    well_alpha: 0.16,
    well_edge: 0.14,
    rim: 0.97,
    chip_alpha: 0.92,
    edge_veil: 0.85,
    glow: 0.017,
};

/// White and graphite: a neutral near-white ground, graphite ink and plates,
/// no hue anywhere but the four bands. The reference's geometry in a
/// monochrome material.
/// The eight colours the bands wear, in band order. They are the reference's
/// own and the same in every palette: a band is told apart by its colour.
pub const BAND_TINTS: [u32; 8] = [
    0xF1768E, 0x5BB2DD, 0x4CA78E, 0xB76BDA, 0xF08C49, 0x8573EB, 0x7C7A8B, 0xF174C5,
];

pub const GRAPHITE: Palette = Palette {
    ground: 0xEEEFF1,
    page: 0xDDDEE1,
    bar: 0xF4F5F6,
    heart: 0xC25064,

    panel_top: 0xF6F7F8,
    panel_bottom: 0xE9EAEC,
    rail_left: 0xDCDDE0,
    rail_right: 0xF0F1F2,
    edge: 0xB4B6BB,
    edge_light: 0xFFFFFF,

    ink: 0x2B2D31,
    ink_quiet: 0x6B6E75,
    ink_logo: 0x2B2D31,
    logo_accent: 0x9A9DA4,
    mark: 0xFFFFFF,

    field: 0xE1E2E5,
    chip: 0xE8E9EB,
    chip_light: 0xEFF0F1,
    toggle: 0xDEDFE2,
    thumb: 0xA3A6AD,
    body: 0x2F3135,
    body_hi: 0x3B3E43,
    selected: 0x2B2D31,
    on_selected: 0xF6F7F8,
    ring_dark: 0xB7BAC0,
    ring_light: 0xDADCE0,

    graph_top: 0xF8F9FA,
    graph_bottom: 0xEEEFF1,
    grid: 0xD8DADE,
    grid_base: 0xC6C8CD,
    fill_top: 0x8A8D94,
    fill_bottom: 0xA9ACB2,
    plot_ink: 0x3A3C41,

    red: 0xC4566B,
    magenta: 0x9550B0,
    cyan: 0x3D7E99,
    green: 0x34856D,

    plate: 0xFFFFFF,
    shade: 0x2B2D31,
    well: 0x5A5C62,
    handle: 0xFFFFFF,
    range_chip: 0xE4E5E8,

    wash: 0.42,
    wash_deep: 0.28,
    veil: 0.16,
    veil_strong: 0.30,
    veil_soft: 0.40,
    shade_alpha: 0.16,
    well_alpha: 0.16,
    well_edge: 0.14,
    rim: 0.97,
    chip_alpha: 0.92,
    edge_veil: 0.85,
    glow: 0.017,
};

/// A look, with the name a person picks it by.
pub struct Theme {
    /// The name on the command line and in `SMOOTH_THEME`.
    pub id: &'static str,
    /// What it is called when it is shown.
    pub name: &'static str,
    /// The colours.
    pub palette: &'static Palette,
}

/// Every look that ships, in the order a picker should show them.
pub const THEMES: &[Theme] = &[
    Theme {
        id: "graphite",
        name: "Graphite",
        palette: &GRAPHITE,
    },
    Theme {
        id: "voice",
        name: "Voice",
        palette: &VOICE,
    },
    Theme {
        id: "console",
        name: "Console",
        palette: &CONSOLE,
    },
    Theme {
        id: "paper",
        name: "Paper",
        palette: &PAPER,
    },
];

/// The default. Parity with the reference is measured against `voice`.
pub const DEFAULT: &Palette = &GRAPHITE;

thread_local! {
    /// Which palette this thread draws with. The editor renders on one
    /// thread, so the look is a property of that thread rather than a
    /// global that two windows would have to share.
    static ACTIVE: Cell<usize> = const { Cell::new(0) };
}

/// The active palette.
///
/// Every part reads this. It is a plain thread-local load, so drawing a
/// control costs one read, and a component has no theme parameter to thread
/// through its callers.
pub fn t() -> &'static Palette {
    let index = ACTIVE.with(|cell| cell.get());
    THEMES
        .get(index)
        .map(|theme| theme.palette)
        .unwrap_or(DEFAULT)
}

/// Pick a look by its id, as `SMOOTH_THEME` or `--theme` names it.
///
/// Returns the theme that is now active, or `None` when the id is not one of
/// [`THEMES`] -- in which case nothing changes.
pub fn set(id: &str) -> Option<&'static Theme> {
    let index = THEMES.iter().position(|theme| theme.id == id)?;
    ACTIVE.with(|cell| cell.set(index));
    THEMES.get(index)
}

/// The look in use.
pub fn active() -> &'static Theme {
    let index = ACTIVE.with(|cell| cell.get());
    THEMES.get(index).unwrap_or(&THEMES[0])
}

/// Pick a look from the environment, if the environment names one.
///
/// A host has no command line of its own, so a plug-in reads `SMOOTH_THEME`;
/// an unknown name is ignored rather than fatal, because a stale environment
/// variable should not stop music.
pub fn from_env() -> Option<&'static Theme> {
    let id = std::env::var("SMOOTH_THEME").ok()?;
    set(id.trim())
}
