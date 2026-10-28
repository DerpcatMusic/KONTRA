//! The editor: a header, the browser, the rack of parts, the keyboard, and
//! panels opened on demand. Built from [`crate::kit`] each frame from the
//! [`App`] the host fills; what the user does comes back as [`Action`]s.
//!
//! One primary action per view: in the browser, pick an instrument; in the
//! rack, play it. Everything informational waits in a tooltip or a panel.

mod browser;
mod rack;

use kontra_ui_model::Interface;
use mui::prelude::*;

use crate::Pictures;
use crate::keyboard::{self, Keys, note_name};
use crate::kit::{self, Icon, button, icon_button, latch, rule, section, sheet, strip, vrule};
use crate::theme::{CONTROL, SIDEBAR, SPACE, TEXT, TIGHT, quick, secondary};

/// Everything the editor shows. The host fills the data; `view` is the
/// editor's own state and survives a reload of the rest.
#[derive(Debug, Default)]
pub struct App {
    pub libraries: Vec<Library>,
    pub parts: Vec<Part>,
    pub stats: Stats,
    /// Master gain in dB.
    pub master: f64,
    /// What did not load or play as the source meant, one line each.
    pub report: Vec<String>,
    pub midi_thru: bool,
    /// The folders the browser scans.
    pub folders: Vec<String>,
    pub view: View,
}

/// A library in the browser.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub name: String,
    pub format: Format,
    pub presets: Vec<String>,
    pub starred: bool,
}

/// Which family of libraries the browser lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Format {
    #[default]
    Kontakt,
    Uvi,
}

/// One loaded instrument.
#[derive(Debug, Default)]
pub struct Part {
    pub name: String,
    pub library: String,
    /// MIDI channel 1..=16, or 0 for every channel.
    pub channel: u8,
    /// Gain in dB, and pan from -1 (left) to 1.
    pub volume: f64,
    pub pan: f64,
    pub solo: bool,
    pub mute: bool,
    /// The articulations and the key that switches to each.
    pub articulations: Vec<(String, Option<u8>)>,
    pub articulation: usize,
    /// The keys it plays.
    pub keys: (u8, u8),
    /// The library's own interface, when it has one.
    pub face: Option<Face>,
    /// Shown below its header.
    pub open: bool,
    pub tab: PartTab,
}

/// A library's interface and its decoded pictures.
pub struct Face {
    pub tree: Interface,
    pub pictures: Pictures,
}

impl std::fmt::Debug for Face {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Face")
            .field("widgets", &self.tree.widgets.len())
            .finish_non_exhaustive()
    }
}

/// A part's views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PartTab {
    #[default]
    Interface,
    Articulations,
}

/// The engine's load, for the header's one readout and its tooltip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub cpu: f64,
    pub voices: u32,
    pub ram_mb: u32,
    pub disk_mbs: f64,
}

/// The editor's own state; its default is a fresh editor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    pub browser_hidden: bool,
    pub format: Format,
    pub starred_only: bool,
    pub search: String,
    /// The library whose presets the browser lists.
    pub library: Option<usize>,
    pub sheet: Option<Sheet>,
    /// The id of the open popup menu.
    pub menu: Option<String>,
    pub keyboard_hidden: bool,
    /// Octaves the keybed is moved from C0 to C5.
    pub octave: i8,
    pub playing: Option<u8>,
    /// Which of [`SCALES`] the editor is drawn at.
    pub scale: usize,
}

/// The editor sizes on offer.
pub const SCALES: [f64; 3] = [1., 1.25, 1.5];

/// A panel opened on demand.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sheet {
    Settings,
    Report,
    About,
}

/// What the user asked for this frame, for the host to carry out.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Load {
        library: usize,
        preset: usize,
    },
    /// The previous (-1) or next (1) preset of a part's library.
    Step {
        part: usize,
        by: i32,
    },
    Volume(usize, f64),
    Pan(usize, f64),
    Solo(usize, bool),
    Mute(usize, bool),
    Channel(usize, u8),
    Articulation(usize, usize),
    Duplicate(usize),
    Remove(usize),
    Master(f64),
    Note(u8, bool),
    Panic,
    MidiThru(bool),
    AddFolder,
    Scale(f64),
}

/// The editor for `app`, and what the user did.
pub fn view(ui: &mut Ui, app: &mut App) -> Response<Vec<Action>> {
    let mut out = Vec::new();
    let header = header(ui, app, &mut out);
    let width = ui.tween(
        "browser-width",
        if app.view.browser_hidden { 0. } else { SIDEBAR },
    );
    let mut middle = Vec::new();
    if width > 0.5 {
        middle.push(browser::view(ui, app, &mut out).w(width).shrink(0).clip());
        middle.push(vrule());
    }
    let mut main = vec![rack::view(ui, app, &mut out).grow(1).min_w(0).min_h(0)];
    if let Some(s) = app.view.sheet {
        main.push(sheet_view(ui, app, s, &mut out).centered());
    }
    middle.push(stack(main).grow(1).min_w(0).min_h(0));
    let el = col![
        header,
        rule(),
        row(middle).grow(1).min_h(0).align(Align::Stretch),
        rule(),
        keys(ui, app, &mut out),
    ]
    .w(Len::Pct(100.))
    .h(Len::Pct(100.))
    .fill(crate::theme::lift(0));
    Response { el, changed: out }
}

/// Opens or closes the popup `id` names on `hit`, and closes it on a click
/// away or Escape; whether it is open.
fn toggle_menu(ui: &Ui, view: &mut View, id: &str, hit: bool) -> bool {
    let open = view.menu.as_deref() == Some(id);
    if hit {
        view.menu = (!open).then(|| id.to_owned());
    } else if open && ui.dismissed(&[id, &format!("{id}-popup")]) {
        view.menu = None;
    }
    view.menu.as_deref() == Some(id)
}

fn header(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let side = icon_button(
        ui,
        "browser",
        Icon::Sidebar,
        "Browser",
        !app.view.browser_hidden,
    );
    if side.changed {
        app.view.browser_hidden = !app.view.browser_hidden;
    }
    let s = app.stats;
    let load = row![
        section("CPU"),
        caption(format!("{:.0}%", s.cpu)).reserve("100%")
    ]
    .gap(TIGHT)
    .align(Align::Center)
    .tip(format!(
        "{} voices · {} MB RAM · {:.1} MB/s from disk",
        s.voices, s.ram_mb, s.disk_mbs
    ))
    .id("load");
    let mut master = app.master;
    let gain = kit::fader(
        ui,
        "master",
        ("Master", &db(master)),
        &mut master,
        -60.0..=6.0,
        0.,
        TEXT * 8.,
    );
    if gain.changed {
        app.master = master;
        out.push(Action::Master(master));
    }
    let panic = icon_button(ui, "panic", Icon::Stop, "Panic: stop every note", false);
    if panic.changed {
        out.push(Action::Panic);
    }
    let mut items = vec![
        side.el,
        body("KONTRA").text_weight(Weight::SEMIBOLD),
        spacer(),
        load,
        vrule().h(CONTROL * 0.6),
        section("Master"),
        gain.el,
        panic.el,
    ];
    if !app.report.is_empty() {
        let n = app.report.len();
        let warn = icon_button(
            ui,
            "warnings",
            Icon::Warning,
            &format!("{n} things did not load as written: open the report"),
            false,
        );
        if warn.changed {
            app.view.sheet = Some(Sheet::Report);
        }
        items.push(warn.el);
    }
    let more = icon_button(ui, "app-menu", Icon::Menu, "Menu", false);
    let mut header = vec![strip(items).grow(1), more.el];
    if toggle_menu(ui, &mut app.view, "app-menu", more.changed) {
        let picked = kit::menu(ui, "app-menu", &["Settings", "Report", "About"], 1, None);
        if let Some(n) = picked.changed {
            app.view.sheet = [Sheet::Settings, Sheet::Report, Sheet::About]
                .get(n)
                .copied();
            app.view.menu = None;
        }
        header.push(picked.el);
    }
    row(header)
        .align(Align::Center)
        .pad(kit::edges(0., SPACE, 0., 0.))
        .shrink(0)
}

fn sheet_view(ui: &mut Ui, app: &mut App, which: Sheet, out: &mut Vec<Action>) -> El {
    let (title, content) = match which {
        Sheet::Settings => ("Settings", settings(ui, app, out)),
        Sheet::Report => ("Report", report(&app.report)),
        Sheet::About => (
            "About",
            col![
                title("KONTRA"),
                caption(
                    "One engine that plays Kontakt, UVI and SFZ libraries as they were written."
                )
                .fill(secondary()),
            ]
            .gap(SPACE),
        ),
    };
    let r = sheet(ui, "sheet", title, content);
    if r.changed {
        app.view.sheet = None;
    }
    r.el
}

fn settings(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let thru = latch(
        ui,
        "midi-thru",
        if app.midi_thru { "On" } else { "Off" },
        "Pass MIDI through to the host",
        app.midi_thru,
    );
    if thru.changed {
        app.midi_thru = !app.midi_thru;
        out.push(Action::MidiThru(app.midi_thru));
    }
    let mut sizes = Vec::new();
    for (n, s) in SCALES.into_iter().enumerate() {
        let b = button(
            ui,
            format!("scale-{n}"),
            &format!("{:.0} %", s * 100.),
            app.view.scale == n,
        );
        if b.changed {
            app.view.scale = n;
            out.push(Action::Scale(s));
        }
        sizes.push(b.el);
    }
    let add = button(ui, "add-folder", "Add folder…", false);
    if add.changed {
        out.push(Action::AddFolder);
    }
    let folders: Vec<El> = app
        .folders
        .iter()
        .map(|f| caption(f.clone()).lines(1).min_w(0).tip(f.clone()))
        .collect();
    let folders = col(folders)
        .gap(TIGHT)
        .align(Align::Start)
        .w(Len::Pct(100.));
    let line = |label: &str, control: El| {
        row![body(label.to_owned()), spacer(), control].align(Align::Center)
    };
    col![
        line("MIDI thru", thru.el),
        line("Size", row(sizes).gap(1)),
        rule(),
        row![section("Library folders"), spacer(), add.el].align(Align::Center),
        folders,
    ]
    .gap(SPACE)
}

fn report(lines: &[String]) -> El {
    if lines.is_empty() {
        return caption("Everything loaded as written.").fill(secondary());
    }
    col(lines
        .iter()
        .map(|l| caption(l.clone()).w(Len::Pct(100.)))
        .collect::<Vec<_>>())
    .gap(SPACE)
    .align(Align::Start)
    .w(Len::Pct(100.))
}

fn keys(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let v = &mut app.view;
    let fold = icon_button(
        ui,
        "keys-fold",
        if v.keyboard_hidden {
            Icon::Right
        } else {
            Icon::Down
        },
        "Keyboard",
        !v.keyboard_hidden,
    );
    if fold.changed {
        v.keyboard_hidden = !v.keyboard_hidden;
    }
    let down = icon_button(ui, "octave-down", Icon::Left, "Octave down", false);
    let up = icon_button(ui, "octave-up", Icon::Right, "Octave up", false);
    if down.changed {
        v.octave = (v.octave - 1).max(-2);
    }
    if up.changed {
        v.octave = (v.octave + 1).min(4);
    }
    let low = (24 + 12 * i32::from(v.octave)).clamp(0, 127 - 71) as u8;
    let range = (low, low + 71);
    let label = caption(format!("{} – {}", note_name(range.0), note_name(range.1)))
        .fill(secondary())
        .tip("Plays every part on its channel, or all of them on Omni")
        .id("keys-range");
    let bar = strip(vec![
        fold.el,
        section("Keyboard"),
        label,
        spacer(),
        down.el,
        up.el,
    ]);
    if v.keyboard_hidden {
        return bar;
    }
    let shown = Keys {
        range,
        held: v.playing.into_iter().collect(),
        spans: app
            .parts
            .iter()
            .enumerate()
            .map(|(n, p)| (p.keys.0, p.keys.1, crate::theme::part_color(n)))
            .collect(),
    };
    let bed = keyboard::keyboard(ui, "keybed", &shown, &mut app.view.playing, TEXT * 6.);
    out.extend(bed.changed.into_iter().map(|(n, on)| Action::Note(n, on)));
    col![bar, row![bed.el].pad(kit::edges(0., SPACE, SPACE, SPACE))].animate_layout_with(quick())
}

/// A gain for a readout.
fn db(v: f64) -> String {
    if v <= -60. {
        "-inf dB".into()
    } else {
        format!("{v:.1} dB")
    }
}
