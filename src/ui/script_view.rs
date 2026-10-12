//! Read-only script inspector. Source stays in memory; only explicit Copy reaches the clipboard.
use super::{Cx, theme::*};
use moose::mui::mui::prelude::*;
use sampler_parity::{ConstructKind, Coverage, Lang, ScriptReport, Span, Status, TokenClass};
use std::{
    fmt::Write,
    ops::Range,
    sync::{Arc, OnceLock, Weak},
};

const ROW: f64 = 22.;
const STATUSES: [Status; 5] = [
    Status::Implemented,
    Status::Approximate,
    Status::Stub,
    Status::Unsupported,
    Status::Fault,
];

/// Build reports and source pairs once when the view binds a loaded instrument.
/// No producer calls belong elsewhere in this view; fixtures are test-only.
fn script_reports(instrument: &sampler_ir::Instrument) -> Vec<(ScriptReport, Arc<str>)> {
    #[cfg(test)]
    if instrument.name == "Synthetic script inspector" {
        return fixtures::reports();
    }
    let mut reports: Vec<_> = sampler_kontakt::parity_reports(instrument)
        .into_iter()
        .zip(
            instrument
                .behaviors
                .iter()
                .filter(|b| b.language == sampler_ir::Language::Ksp),
        )
        .map(|(report, behavior)| (report, Arc::from(behavior.source.as_str())))
        .collect();
    if let Some(source) = &instrument.uvi_script_sources {
        reports.extend(sampler_uvi::parity::report_sources(
            &source.xml,
            &sampler_uvi::script::Scripts::from_sources(source.files.clone()),
        ));
    }
    reports
}

/// Coordinator seam: idempotently enable/disable collection and read the latest snapshot.
/// `None` means unavailable, never an observed zero-hit execution.
fn script_coverage(
    _instrument: &sampler_ir::Instrument,
    _id: &str,
    _collect: bool,
) -> Option<Coverage> {
    #[cfg(test)]
    if _instrument.name == "Synthetic script inspector" && _collect {
        return fixtures::reports()
            .into_iter()
            .find(|(r, _)| r.id == _id)
            .map(|(r, s)| fixtures::coverage(&Script::new(r, s)));
    }
    // shortcut: KSP's evaluator is not the playback VM and Luau needs recompilation;
    // add owner-thread/VM observation before enabling coverage without resetting live notes.
    None
}

pub(super) fn mono() -> Font {
    static FONT: OnceLock<Font> = OnceLock::new();
    FONT.get_or_init(|| {
        Font::new(&include_bytes!("script_fonts/DejaVuSansMono.ttf")[..])
            .expect("bundled DejaVu Sans Mono")
    })
    .clone()
}
fn language(lang: Lang) -> &'static str {
    match lang {
        Lang::Ksp => "KSP",
        Lang::Luau => "Luau",
    }
}
fn status(status: Status) -> (&'static str, &'static str, Color) {
    match status {
        Status::Implemented => ("✓", "Binding available", Color::srgb(0.55, 0.85, 0.65)),
        Status::Approximate => ("≈", "Approximate", Color::srgb(0.52, 0.78, 1.)),
        Status::Stub => ("○", "Stub", Color::srgb(0.8, 0.65, 1.)),
        Status::Unsupported => ("✗", "Unsupported", Color::srgb(1., 0.51, 0.48)),
        Status::Fault => ("⚠", "Fault", Color::srgb(1., 0.59, 0.82)),
    }
}
fn syntax(class: TokenClass) -> Color {
    match class {
        TokenClass::Keyword | TokenClass::Preprocessor => Color::srgb(0.8, 0.65, 1.),
        TokenClass::Callback | TokenClass::Builtin => Color::srgb(0.52, 0.78, 1.),
        TokenClass::Variable => Color::srgb(0.74, 0.88, 0.98),
        TokenClass::Constant | TokenClass::Number => Color::srgb(1., 0.69, 0.87),
        TokenClass::String => Color::srgb(0.61, 0.86, 0.69),
        TokenClass::Comment => Color::srgb(0.64, 0.67, 0.69),
        _ => Color::srgb(0.91, 0.91, 0.91),
    }
}

struct Line {
    bytes: Range<usize>,
    status: Option<Status>,
    tokens: Vec<usize>,
    constructs: Vec<usize>,
    lowered: Vec<usize>,
    hits: u64,
    fault: bool,
}
struct Script {
    report: ScriptReport,
    source: Arc<str>,
    lines: Vec<Line>,
    shares: [usize; 5],
    coverage: Option<Coverage>,
    fault_starts: Vec<u32>,
    revision: u64,
    columns: usize,
}
impl Script {
    fn new(report: ScriptReport, source: Arc<str>) -> Self {
        let mut start = 0;
        let mut lines = Vec::new();
        for text in source.split('\n') {
            let end = start + text.trim_end_matches('\r').len();
            lines.push(Line {
                bytes: start..end,
                status: None,
                tokens: vec![],
                constructs: vec![],
                lowered: vec![],
                hits: 0,
                fault: false,
            });
            start += text.len() + 1;
        }
        let mut out = Self {
            report,
            source,
            lines,
            shares: [0; 5],
            coverage: None,
            fault_starts: Vec::new(),
            revision: 0,
            columns: 0,
        };
        for (line, status) in out.report.line_status() {
            if let Some(l) = line
                .checked_sub(1)
                .and_then(|n| out.lines.get_mut(n as usize))
            {
                l.status = Some(status);
            }
        }
        // Index once, including multi-line tokens, instead of scanning the report per visible row.
        for (n, token) in out.report.tokens.iter().enumerate() {
            let span = token.span;
            if out.valid(span) {
                let first = out.line_at(span.start as usize);
                let last = out.line_at(span.end.saturating_sub(1) as usize);
                for line in &mut out.lines[first..=last] {
                    line.tokens.push(n);
                }
            }
        }
        for line in &mut out.lines {
            line.tokens
                .sort_unstable_by_key(|&n| out.report.tokens[n].span.start);
        }
        for (n, c) in out.report.constructs.iter().enumerate() {
            out.shares[c.status as usize] += 1;
            if out.valid(c.span) {
                let first = out.line_at(c.span.start as usize);
                let last = out.line_at(c.span.end.saturating_sub(1) as usize);
                for line in &mut out.lines[first..=last] {
                    line.constructs.push(n);
                }
            }
        }
        for (n, l) in out.report.lowered.iter().enumerate() {
            if out.valid(l.span) {
                let first = out.line_at(l.span.start as usize);
                let last = out.line_at(l.span.end.saturating_sub(1) as usize);
                for line in &mut out.lines[first..=last] {
                    line.lowered.push(n);
                }
            }
        }
        out.columns = (0..out.lines.len())
            .map(|n| columns(out.text(n), 0))
            .max()
            .unwrap_or(0);
        out
    }
    fn valid(&self, span: Span) -> bool {
        span.start < span.end
            && span.end as usize <= self.source.len()
            && self.source.is_char_boundary(span.start as usize)
            && self.source.is_char_boundary(span.end as usize)
    }
    fn line_at(&self, byte: usize) -> usize {
        self.lines
            .partition_point(|l| l.bytes.start <= byte)
            .saturating_sub(1)
    }
    fn text(&self, line: usize) -> &str {
        &self.source[self.lines[line].bytes.clone()]
    }
    fn line_status(&self, line: usize) -> Option<Status> {
        if self.lines[line].fault {
            Some(Status::Fault)
        } else {
            self.lines[line].status
        }
    }
    fn construct_status(&self, n: usize) -> Status {
        let c = &self.report.constructs[n];
        if self.fault_starts.binary_search(&c.span.start).is_ok() {
            Status::Fault
        } else {
            c.status
        }
    }
    fn observe(&mut self, coverage: Option<Coverage>) {
        let coverage = coverage.filter(|c| c.script_id == self.report.id);
        if self.coverage == coverage {
            return;
        }
        self.fault_starts = coverage.as_ref().map_or_else(Vec::new, |c| {
            c.faults
                .iter()
                .filter_map(|(byte, _)| {
                    ((*byte as usize) < self.source.len()
                        && self.source.is_char_boundary(*byte as usize))
                    .then_some(*byte)
                })
                .collect()
        });
        self.fault_starts.sort_unstable();
        self.fault_starts.dedup();
        for l in &mut self.lines {
            l.hits = 0;
            l.fault = false;
        }
        if let Some(c) = &coverage {
            for &(byte, hits) in &c.hits {
                if (byte as usize) < self.source.len()
                    && self.source.is_char_boundary(byte as usize)
                {
                    let n = self.line_at(byte as usize);
                    self.lines[n].hits = self.lines[n].hits.saturating_add(hits);
                }
            }
            for (byte, _) in &c.faults {
                if (*byte as usize) < self.source.len()
                    && self.source.is_char_boundary(*byte as usize)
                {
                    let n = self.line_at(*byte as usize);
                    self.lines[n].fault = true;
                }
            }
        }
        self.coverage = coverage;
        self.shares = [0; 5];
        for n in 0..self.report.constructs.len() {
            self.shares[self.construct_status(n) as usize] += 1;
        }
        self.revision = self.revision.wrapping_add(1);
    }
}

pub(super) struct State {
    instrument: Weak<sampler_ir::Instrument>,
    scripts: Vec<Script>,
    selected: usize,
    line: usize,
    construct: Option<usize>,
    search: String,
    gaps: bool,
    enabled: [bool; 5],
    filter: Option<(usize, String, bool, [bool; 5], u64)>,
    inspection: Option<((usize, usize, Option<usize>, bool, u64), [Arc<str>; 3])>,
    matches: Vec<usize>,
    y: f64,
    x: f64,
    reveal: bool,
    live: bool,
    collecting: Option<String>,
    callback: bool,
    message: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            instrument: Weak::new(),
            scripts: vec![],
            selected: 0,
            line: 0,
            construct: None,
            search: String::new(),
            gaps: false,
            enabled: [true; 5],
            filter: None,
            inspection: None,
            matches: vec![],
            y: 0.,
            x: 0.,
            reveal: false,
            live: false,
            collecting: None,
            callback: false,
            message: String::new(),
        }
    }
}
impl State {
    pub(super) fn collecting(&self) -> bool {
        self.collecting.is_some()
    }
    pub(super) fn stop_coverage(&mut self) {
        if let Some(id) = self.collecting.take()
            && let Some(i) = self.instrument.upgrade()
        {
            script_coverage(&i, &id, false);
        }
    }
    fn select(&mut self, selected: usize) {
        if selected == self.selected {
            return;
        }
        self.stop_coverage();
        self.selected = selected;
        self.line = 0;
        self.construct = None;
        self.x = 0.;
        self.y = 0.;
        self.filter = None;
    }
    fn filter(&mut self) {
        let script = &self.scripts[self.selected];

        let key = (
            self.selected,
            self.search.clone(),
            self.gaps,
            self.enabled,
            script.revision,
        );
        if self.filter.as_ref() == Some(&key) {
            return;
        }
        let query = self.search.to_lowercase();
        self.matches = script
            .lines
            .iter()
            .enumerate()
            .filter_map(|(n, _)| {
                let s = script.line_status(n);
                (s.is_none_or(|s| self.enabled[s as usize])
                    && (!self.gaps || s.is_some_and(|s| s != Status::Implemented))
                    && (query.is_empty() || script.text(n).to_lowercase().contains(&query)))
                .then_some(n)
            })
            .collect();
        self.filter = Some(key);
        self.y = 0.;
        if !self.matches.contains(&self.line)
            && let Some(&n) = self.matches.first()
        {
            self.line = n;
            self.construct = None;
        }
    }
    fn jump(&mut self, forward: bool, gaps: bool) {
        let script = &self.scripts[self.selected];
        let candidates: Vec<_> = self
            .matches
            .iter()
            .copied()
            .filter(|&n| {
                !gaps
                    || script
                        .line_status(n)
                        .is_some_and(|s| s != Status::Implemented)
            })
            .collect();
        let n = if forward {
            candidates
                .iter()
                .copied()
                .find(|&n| n > self.line)
                .or_else(|| candidates.first().copied())
        } else {
            candidates
                .iter()
                .rev()
                .copied()
                .find(|&n| n < self.line)
                .or_else(|| candidates.last().copied())
        };
        if let Some(n) = n {
            self.line = n;
            self.construct = None;
            self.reveal = true;
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.stop_coverage();
    }
}

pub(super) fn view(
    ui: &mut Ui,
    cx: &mut Cx,
    slot: usize,
    instrument: &Arc<sampler_ir::Instrument>,
) -> El {
    let state = &mut cx.state.inside.entry(slot).or_default().scripts;
    if !Weak::ptr_eq(&state.instrument, &Arc::downgrade(instrument)) {
        state.stop_coverage();
        *state = State::default();
        state.instrument = Arc::downgrade(instrument);
        state.scripts = script_reports(instrument)
            .into_iter()
            .map(|(r, s)| Script::new(r, s))
            .collect();
    }
    let height = ui
        .scene()
        .and_then(|s| s.surface("rack-view"))
        .map_or(460., |s| (s.frame.size.height - 100.).clamp(360., 640.));
    draw(ui, state, &format!("scripts-{slot}")).h(height)
}

fn summary(script: &Script) -> El {
    let total: usize = script.shares.iter().sum();
    let mut bars = Vec::new();
    for s in STATUSES {
        let count = script.shares[s as usize];
        if count > 0 {
            bars.push(
                block(Len::Pct(count as f64 * 100. / total as f64), 3.)
                    .fill(status(s).2)
                    .tip(format!("{}: {count}/{total} construct uses", status(s).1))
                    .shrink(0),
            );
        }
    }
    row(bars).gap(0).h(3.).fill(Role::Raised)
}

fn draw(ui: &mut Ui, state: &mut State, key: &str) -> El {
    if state.scripts.is_empty() {
        return col![
            section("Scripts"),
            body("Script reports are not available for this instrument yet.")
                .fill(secondary())
                .lines(3)
        ]
        .gap(SPACE)
        .pad(INSET)
        .align(Align::Start)
        .id(key);
    }
    let mut scripts = Vec::new();
    for n in 0..state.scripts.len() {
        let id = format!("{key}-script-{n}");
        if ui.get(id.as_str()).activated() {
            state.select(n);
        }
        let script = &state.scripts[n];
        let gaps: usize = script.shares[1..].iter().sum();
        scripts.push(interactive(
            col![
                body(script.report.title.clone()).text_size(TEXT).lines(2),
                caption(format!(
                    "{} · {gaps}/{} gaps",
                    language(script.report.lang),
                    script.report.constructs.len()
                ))
                .fill(secondary())
                .lines(1),
                summary(script)
            ]
            .gap(TIGHT)
            .pad(SPACE)
            .align(Align::Stretch)
            .shrink(0)
            .when(n == state.selected, |e| e.fill(Role::Raised))
            .focusable()
            .a11y(A11y::Button)
            .named(format!(
                "{} {}",
                script.report.title,
                language(script.report.lang)
            ))
            .id(id),
            n == state.selected,
        ));
    }
    let (copy, copy_el) = action(ui, format!("{key}-copy"), "Copy gaps for AI", false);
    if copy {
        ui.set_clipboard(copy_gaps(&state.scripts));
        state.message = "Copied gaps to clipboard.".into();
    }
    let (live, live_el) = latch(
        ui,
        format!("{key}-live"),
        "Live coverage",
        "Collect script execution coverage",
        state.live,
    );
    if live {
        state.live ^= true;
        if !state.live {
            state.stop_coverage();
        }
    }
    if state.live
        && let Some(i) = state.instrument.upgrade()
    {
        let id = &state.scripts[state.selected].report.id;
        let coverage = script_coverage(&i, id, true);
        state.collecting = Some(id.clone());
        state.scripts[state.selected].observe(coverage);
    }
    let field = super::browser::search_field(
        ui,
        &format!("{key}-search"),
        &mut state.search,
        "Search source",
        "Search script source",
    )
    .min_w(100.)
    .flex(1)
    .tip("Search source; Next match walks matching lines");
    let (gaps, gaps_el) = latch(
        ui,
        format!("{key}-gaps"),
        "Only gaps",
        "Show binding gaps and recorded faults",
        state.gaps,
    );
    if gaps {
        state.gaps ^= true;
    }
    let mut filters = vec![gaps_el];
    for s in STATUSES {
        let (hit, el) = latch(
            ui,
            format!("{key}-status-{s:?}"),
            status(s).0,
            status(s).1,
            state.enabled[s as usize],
        );
        if hit {
            state.enabled[s as usize] ^= true;
        }
        filters.push(el.tip(format!("{} {}", status(s).0, status(s).1)));
    }
    let (prev, prev_el) = action(ui, format!("{key}-prev"), "Prev gap", false);
    let (next, next_el) = action(ui, format!("{key}-next"), "Next gap", false);
    let (find, find_el) = action(ui, format!("{key}-find"), "Next match", false);
    filters.extend([prev_el, next_el, find_el]);
    state.filter();
    if prev {
        state.jump(false, true);
    }
    if next {
        state.jump(true, true);
    }
    if find {
        state.jump(true, false);
    }
    let code = code(ui, state, key);
    let details = inspector(ui, state, key);
    let coverage = if state.scripts[state.selected].coverage.is_some() {
        "Coverage observed · hit counts are statement totals"
    } else if state.live {
        "Coverage requested · producer not connected"
    } else {
        "Coverage not observed"
    };
    row![
        col![
            section("Scripts"),
            col(scripts)
                .gap(TIGHT)
                .align(Align::Stretch)
                .flex(1)
                .min_h(0)
                .scroll(),
            caption(state.message.clone()).fill(secondary()).lines(2),
            copy_el,
            live_el
        ]
        .gap(SPACE)
        .align(Align::Stretch)
        .w(180.)
        .min_w(140.)
        .pad(SPACE),
        vrule(),
        col![
            row![field].pad((SPACE, TIGHT)).shrink(0),
            row(filters).gap(TIGHT).wrap().pad((SPACE, TIGHT)).shrink(0),
            caption("Binding status · native behavior requires separate evidence")
                .fill(secondary())
                .pad((SPACE, TIGHT))
                .shrink(0),
            code,
            caption(coverage)
                .fill(secondary())
                .pad((SPACE, TIGHT))
                .shrink(0),
            rule(),
            details
        ]
        .gap(0)
        .align(Align::Stretch)
        .flex(1)
        .min_w(0)
    ]
    .gap(0)
    .align(Align::Stretch)
    .min_w(0)
    .id(key)
}

fn visible(count: usize, y: f64, height: f64) -> Range<usize> {
    let first = ((y / ROW).floor() as usize).min(count);
    first..(((y + height) / ROW).ceil() as usize + 1).min(count)
}

fn code(ui: &mut Ui, state: &mut State, key: &str) -> El {
    let id = format!("{key}-code");
    let bar = format!("{id}-bar");
    let size = ui
        .scene()
        .and_then(|s| s.surface(&id))
        .map_or(Size::new(600., 200.), |s| s.frame.size);
    let height = size.height.max(ROW);
    let total = state.matches.len() as f64 * ROW;
    if let Some(wheel) = ui.wheel(id.as_str()) {
        state.y += wheel.y;
        state.x = (state.x + wheel.x).max(0.);
    }
    if ui.focus_key().is_some_and(|focus| {
        focus == id
            || focus.starts_with(&format!("{key}-line-"))
            || focus.starts_with(&format!("{key}-construct-"))
    }) {
        for press in ui.shortcuts() {
            match press.key {
                Key::Down => state.jump(true, false),
                Key::Up => state.jump(false, false),
                Key::Home | Key::End => {
                    let n = if press.key == Key::Home {
                        state.matches.first()
                    } else {
                        state.matches.last()
                    };
                    if let Some(&n) = n {
                        state.line = n;
                        state.construct = None;
                        state.reveal = true;
                    }
                }
                Key::PageDown => state.y += height,
                Key::PageUp => state.y -= height,
                Key::Left => state.x = (state.x - 80.).max(0.),
                Key::Right => state.x += 80.,
                _ => {}
            }
        }
    }
    bar_drag(ui, &bar, &mut state.y, height, total);
    if state.reveal {
        if let Some(n) = state.matches.iter().position(|&n| n == state.line) {
            let top = n as f64 * ROW;
            if top < state.y {
                state.y = top;
            } else if top + ROW > state.y + height {
                state.y = top + ROW - height;
            }
        }
        state.reveal = false;
    }
    state.y = state.y.clamp(0., (total - height).max(0.));
    let range = visible(state.matches.len(), state.y, height);
    let font = mono();
    let cell = ui.text_run(&font, "M", TEXT).map_or(7.2, |r| r.advance);
    let mut rows = vec![block(1., range.start as f64 * ROW).shrink(0)];
    let mut hover = None;
    state.x = state.x.min(
        (state.scripts[state.selected].columns as f64 * cell - (size.width - 116.).max(1.)).max(0.),
    );
    for rank in range.clone() {
        let n = state.matches[rank];
        let script = &state.scripts[state.selected];
        let line = &script.lines[n];
        let line_id = format!("{key}-line-{}", n + 1);
        if ui.get(line_id.as_str()).activated() {
            state.line = n;
            state.construct = None;
        }
        let mark = script.line_status(n);
        let gutter = mark.map_or(" ", |s| status(s).0);
        let ink = mark.map_or(secondary(), |s| Fill::from(status(s).2));
        let hits = if script.coverage.is_some() {
            line.hits.to_string()
        } else {
            "—".into()
        };
        let mut layers = vec![paint_line(
            ui,
            script,
            n,
            state.x,
            (size.width - 116.).max(1.),
            &font,
            cell,
        )];
        for &c in &line.constructs {
            let construct = &script.report.constructs[c];
            let c_status = script.construct_status(c);
            let start = (construct.span.start as usize).max(line.bytes.start);
            let end = (construct.span.end as usize).min(line.bytes.end);
            if start >= end {
                continue;
            }
            let column = columns(&script.source[line.bytes.start..start], 0);
            let x = column as f64 * cell - state.x;
            let width = columns(&script.source[start..end], column) as f64 * cell;
            if x + width <= 0. || x >= size.width - 116. {
                continue;
            }
            let c_id = format!("{key}-construct-{c}-{}", n + 1);
            let response = ui.get(c_id.as_str());
            if response.activated() {
                state.line = n;
                state.construct = Some(c);
            }
            if response.hovered {
                hover = Some((n, c));
            }
            layers.push(
                block(width, ROW)
                    .at(x, 0.)
                    .focusable()
                    .a11y(A11y::Button)
                    .named(format!("{}: {}", construct.name, status(c_status).1))
                    .tip(format!(
                        "{} · {}\n{}",
                        construct.name,
                        status(c_status).1,
                        construct.detail
                    ))
                    .id(c_id),
            );
        }
        rows.push(
            row![
                caption((n + 1).to_string())
                    .font(font.clone())
                    .fill(secondary())
                    .w(44.)
                    .lines(1),
                caption(gutter)
                    .fill(ink)
                    .w(18.)
                    .lines(1)
                    .named(mark.map_or("No construct", |s| status(s).1))
                    .id(format!("{key}-gutter-{}-{mark:?}", n + 1)),
                caption(hits)
                    .font(font.clone())
                    .fill(secondary())
                    .w(42.)
                    .lines(1)
                    .tip("Executions of statements on this line"),
                stack(layers).flex(1).min_w(0).h(ROW).clip()
            ]
            .gap(TIGHT)
            .align(Align::Center)
            .h(ROW)
            .shrink(0)
            .clip()
            .when(state.line == n, |e| e.fill(Role::Raised))
            .focusable()
            .a11y(A11y::Button)
            .named(format!("Line {}: {}", n + 1, script.text(n)))
            .id(line_id),
        );
    }
    if let Some((n, c)) = hover {
        state.line = n;
        state.construct = Some(c);
    }
    rows.push(block(1., (state.matches.len() - range.end) as f64 * ROW).shrink(0));
    if state.matches.is_empty() {
        rows.push(
            body("No lines match these filters.")
                .fill(secondary())
                .pad(INSET),
        );
    }
    let list = col(rows)
        .gap(0)
        .align(Align::Stretch)
        .h(Len::Pct(100.))
        .w(Len::Pct(100.))
        .scroll()
        .no_scrollbar()
        .scrolled(0., state.y)
        .captures_wheel()
        .focusable()
        .a11y(A11y::Group)
        .named("Read-only script source; arrows navigate, left/right scroll horizontally")
        .id(id);
    let mut layers = vec![list];
    if total > height {
        layers.push(
            scrollbar(ui, &bar, "Scroll script lines", state.y, height, total)
                .anchor(Align::End, Align::Start),
        );
    }
    stack(layers).flex(1).min_h(80.).min_w(0).fill(Role::Field)
}

fn columns(text: &str, start: usize) -> usize {
    text.chars()
        .fold(start, |n, c| n + if c == '\t' { 4 - n % 4 } else { 1 })
        - start
}

fn paint_line(
    ui: &mut Ui,
    script: &Script,
    n: usize,
    x: f64,
    width: f64,
    font: &Font,
    cell: f64,
) -> El {
    let line = &script.lines[n];
    let first = (x / cell).floor() as usize;
    let last = ((x + width) / cell).ceil() as usize;
    let mut col = 0;
    let mut token = 0;
    let mut runs: Vec<(usize, TokenClass, String)> = Vec::new();
    for (byte, ch) in script.text(n).char_indices() {
        let at = line.bytes.start + byte;
        while token < line.tokens.len()
            && script.report.tokens[line.tokens[token]].span.end as usize <= at
        {
            token += 1;
        }
        let class = line
            .tokens
            .get(token)
            .map(|&t| script.report.tokens[t])
            .filter(|t| t.span.start as usize <= at)
            .map_or(TokenClass::Other, |t| t.class);
        let advance = if ch == '\t' { 4 - col % 4 } else { 1 };
        if col > last {
            break;
        }
        if col + advance > first {
            let shown = if ch == '\t' {
                " ".repeat(advance)
            } else {
                ch.to_string()
            };
            if let Some((_, _, text)) = runs.last_mut().filter(|(_, c, _)| *c == class) {
                text.push_str(&shown);
            } else {
                runs.push((col, class, shown));
            }
        }
        col += advance;
    }
    let alpha = if script.coverage.is_some() && line.hits == 0 {
        0.52
    } else {
        1.
    };
    let mut draws = Vec::new();
    for (at, class, text) in runs {
        if let Some(run) = ui.text_run(font, &text, TEXT) {
            draws.push(
                Draw::fill(run.path.clone(), syntax(class).with_alpha(alpha))
                    .at(Point::new(at as f64 * cell - x, 3. + run.ascent)),
            );
        }
    }
    for &c in &line.constructs {
        let c_status = script.construct_status(c);
        let c = &script.report.constructs[c];
        if c_status == Status::Implemented {
            continue;
        }
        let start = (c.span.start as usize).max(line.bytes.start);
        let end = (c.span.end as usize).min(line.bytes.end);
        if start < end {
            let at = columns(&script.source[line.bytes.start..start], 0);
            let len = columns(&script.source[start..end], at);
            let left = (at as f64 * cell - x).max(0.);
            let right = ((at + len) as f64 * cell - x).min(width);
            if right > left {
                draws.push(Draw::fill(
                    rect(left, ROW - 3., right - left, 1.),
                    status(c_status).2,
                ));
            }
        }
    }
    canvas(move |_| draws.clone()).h(ROW).w(Len::Pct(100.))
}

fn construct_detail(script: &Script, n: usize) -> String {
    let c = &script.report.constructs[n];
    let c_status = script.construct_status(n);
    let mut out = format!(
        "{} · {:?} · {} {}\n{}\nOwner: {}\nSpec: {}",
        c.name,
        c.kind,
        status(c_status).0,
        status(c_status).1,
        c.detail,
        c.owner.as_deref().unwrap_or("Not provided"),
        c.spec.as_deref().unwrap_or("Not provided")
    );
    if let Some(coverage) = &script.coverage {
        for (_, fault) in coverage
            .faults
            .iter()
            .filter(|(byte, _)| *byte == c.span.start)
        {
            let _ = write!(out, "\nRuntime fault: {fault}");
        }
    }
    for lowered in &script.report.lowered {
        if overlaps(c.span, lowered.span) {
            for op in &lowered.ops {
                let _ = write!(out, "\n→ {op}");
            }
        }
    }
    out
}
fn overlaps(a: Span, b: Span) -> bool {
    a.start < b.end && b.start < a.end
}

fn inspector(ui: &mut Ui, state: &mut State, key: &str) -> El {
    let (hit, callback) = latch(
        ui,
        format!("{key}-callback"),
        "Callback",
        "Show output for the enclosing callback span",
        state.callback,
    );
    if hit {
        state.callback ^= true;
    }
    let script = &state.scripts[state.selected];
    let line = &script.lines[state.line];
    let picked = state.construct.or_else(|| line.constructs.first().copied());
    let cache_key = (
        state.selected,
        state.line,
        picked,
        state.callback,
        script.revision,
    );
    if state
        .inspection
        .as_ref()
        .is_none_or(|(k, _)| *k != cache_key)
    {
        let span = if state.callback {
            script
                .report
                .constructs
                .iter()
                .find(|c| {
                    c.kind == ConstructKind::Callback
                        && c.span.start as usize <= line.bytes.start
                        && c.span.end as usize >= line.bytes.end
                })
                .map(|c| c.span)
        } else {
            None
        }
        .unwrap_or(Span {
            start: line.bytes.start as u32,
            end: line.bytes.end as u32,
            line: state.line as u32 + 1,
        });
        let mut out = format!("Line {}\n{}\n\n", state.line + 1, script.text(state.line));
        let mut count = 0;
        for lowered in &script.report.lowered {
            if overlaps(span, lowered.span)
                || (!state.callback && lowered.span.line == state.line as u32 + 1)
            {
                for op in &lowered.ops {
                    let _ = writeln!(out, "{} → {op}", lowered.span.line);
                    count += 1;
                }
            }
        }
        if count == 0 {
            out.push_str("No lowered operations reported for this selection.");
        }
        let detail = picked.map_or_else(
            || "Select or hover a construct to inspect its status, owner and spec.".into(),
            |c| construct_detail(script, c),
        );
        let faults = script.coverage.as_ref().map_or_else(
            || "No coverage observed.".into(),
            |c| {
                if c.faults.is_empty() {
                    "No runtime faults observed.".into()
                } else {
                    c.faults
                        .iter()
                        .map(|(byte, fault)| {
                            format!("Line {}: {fault}", script.line_at(*byte as usize) + 1)
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            },
        );
        state.inspection = Some((cache_key, [detail.into(), out.into(), faults.into()]));
    }
    let [detail, out, faults] = state.inspection.as_ref().unwrap().1.clone();
    col![
        section_bar("In → Out", vec![callback]),
        row![
            col![section("Construct"), body(detail).text_size(TEXT).shrink(0)]
                .gap(TIGHT)
                .pad(SPACE)
                .align(Align::Stretch)
                .flex(1)
                .min_w(0)
                .scroll()
                .captures_wheel()
                .id(format!("{key}-detail")),
            vrule(),
            col![
                body(out).font(mono()).text_size(TEXT).shrink(0),
                section("Faults"),
                body(faults).text_size(TEXT).shrink(0)
            ]
            .gap(SPACE)
            .pad(SPACE)
            .align(Align::Stretch)
            .flex(1)
            .min_w(0)
            .scroll()
            .captures_wheel()
            .id(format!("{key}-output"))
        ]
        .align(Align::Stretch)
        .flex(1)
        .min_h(0)
    ]
    .gap(0)
    .align(Align::Stretch)
    .h(160.)
    .shrink(0)
}

fn markdown(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('`', "\\`")
        .replace('|', "\\|")
        .replace('\n', " ")
        .replace('\r', " ")
}
fn copy_gaps(scripts: &[Script]) -> String {
    let mut out = String::new();
    for script in scripts {
        let gaps: Vec<_> = script
            .report
            .constructs
            .iter()
            .filter(|c| c.status != Status::Implemented)
            .collect();
        if gaps.is_empty() && script.coverage.as_ref().is_none_or(|c| c.faults.is_empty()) {
            continue;
        }
        let _ = writeln!(
            out,
            "## {} ({})",
            markdown(&script.report.id),
            language(script.report.lang)
        );
        for c in gaps {
            let text = c
                .span
                .line
                .checked_sub(1)
                .and_then(|n| script.lines.get(n as usize))
                .map_or("Source line unavailable", |l| {
                    &script.source[l.bytes.clone()]
                });
            let _ = writeln!(
                out,
                "- L{} **{}** [{}]: {} · owner: {} · spec: {}\n  > {}",
                c.span.line,
                markdown(&c.name),
                status(c.status).1,
                markdown(&c.detail),
                markdown(c.owner.as_deref().unwrap_or("Not provided")),
                markdown(c.spec.as_deref().unwrap_or("Not provided")),
                markdown(text)
            );
        }
        if let Some(coverage) = &script.coverage {
            for (byte, fault) in &coverage.faults {
                if (*byte as usize) >= script.source.len() {
                    continue;
                }
                let n = script.line_at(*byte as usize);
                let construct = script.lines[n]
                    .constructs
                    .iter()
                    .map(|&n| &script.report.constructs[n])
                    .filter(|c| c.span.start <= *byte && *byte < c.span.end)
                    .min_by_key(|c| c.span.end - c.span.start);
                let _ = writeln!(
                    out,
                    "- L{} **{}** [Fault]: {} · owner: {} · spec: {}\n  > {}",
                    n + 1,
                    markdown(construct.map_or("runtime", |c| c.name.as_str())),
                    markdown(fault),
                    markdown(
                        construct
                            .and_then(|c| c.owner.as_deref())
                            .unwrap_or("Not provided")
                    ),
                    markdown(
                        construct
                            .and_then(|c| c.spec.as_deref())
                            .unwrap_or("Not provided")
                    ),
                    markdown(script.text(n))
                );
            }
        }
    }
    if out.is_empty() {
        out.push_str("No script gaps reported.");
    }
    out
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use sampler_parity::{Construct, Lowered, Token};

    pub const KSP: &str = "on init\n  declare $gain := 0\n  set_engine_par($ENGINE_PAR_VOLUME, $gain, -1, -1, -1)\n  set_control_par($gain, $CONTROL_PAR_HIDE, 1)\n  load_native_ui(\"panel.nckp\")\n  message(\"synthetic fault\")\nend on\non note\n  play_note($EVENT_NOTE, 100, 0, -1)\nend on";
    pub const LUAU: &str = "local dsp = require(\"SyntheticDSP\")\nfunction onNote(event)\n  event:setParameter(\"Pitch\", 0)\n  dsp.process(event)\n  print(\"synthetic fault\")\nend";

    fn span(source: &str, text: &str) -> Span {
        let start = source.find(text).unwrap();
        Span {
            start: start as u32,
            end: (start + text.len()) as u32,
            line: source[..start].bytes().filter(|&b| b == b'\n').count() as u32 + 1,
        }
    }
    fn fixture(
        lang: Lang,
        source: &str,
        uses: &[(&str, Status, ConstructKind)],
        tokens: &[(&str, TokenClass)],
    ) -> (ScriptReport, Arc<str>) {
        let id = match lang {
            Lang::Ksp => "ksp:slot0",
            Lang::Luau => "uvi:/Program/EventProcessors/0",
        };
        let mut constructs: Vec<_> = uses
            .iter()
            .map(|&(name, status, kind)| Construct {
                span: span(source, name),
                kind,
                name: name.into(),
                status,
                detail: match status {
                    Status::Implemented => "Synthetic tested binding",
                    Status::Approximate => "Synthetic approximation differs in timing",
                    Status::Stub => "Synthetic accepted call has no effect",
                    Status::Unsupported => "Synthetic binding is not lowered",
                    Status::Fault => "Synthetic runtime fault",
                }
                .into(),
                owner: Some("fixture/host.rs:42".into()),
                spec: Some("fixture/SCRIPT_SPEC.md#binding".into()),
            })
            .collect();
        if let Some(c) = constructs
            .iter_mut()
            .find(|c| c.kind == ConstructKind::Callback)
        {
            c.span.end = if lang == Lang::Ksp {
                source.find("end on").unwrap() + 6
            } else {
                source.len()
            } as u32;
        }
        let lowered = constructs
            .iter()
            .filter(|c| c.status <= Status::Approximate)
            .map(|c| Lowered {
                span: c.span,
                ops: vec![format!("host.bind({})", c.name)],
            })
            .collect();
        (
            ScriptReport {
                id: id.into(),
                title: match lang {
                    Lang::Ksp => "Kontakt · synthetic fixture",
                    Lang::Luau => "Falcon · synthetic fixture",
                }
                .into(),
                lang,
                source_len: source.len() as u32,
                tokens: tokens
                    .iter()
                    .map(|&(text, class)| Token {
                        span: span(source, text),
                        class,
                    })
                    .collect(),
                constructs,
                lowered,
            },
            source.into(),
        )
    }
    pub fn reports() -> Vec<(ScriptReport, Arc<str>)> {
        use ConstructKind::*;
        use Status::*;
        use TokenClass as T;
        vec![
            fixture(
                Lang::Ksp,
                KSP,
                &[
                    ("on init", Implemented, Callback),
                    ("declare", Implemented, Syntax),
                    ("set_engine_par", Approximate, Builtin),
                    ("set_control_par", Stub, Builtin),
                    ("load_native_ui", Unsupported, Builtin),
                    ("message", Fault, Builtin),
                    ("play_note", Implemented, Builtin),
                ],
                &[
                    ("on init", T::Callback),
                    ("declare", T::Keyword),
                    ("$gain", T::Variable),
                    ("0", T::Number),
                    ("set_engine_par", T::Builtin),
                    ("$ENGINE_PAR_VOLUME", T::Constant),
                    ("set_control_par", T::Builtin),
                    ("$CONTROL_PAR_HIDE", T::Constant),
                    ("load_native_ui", T::Builtin),
                    ("\"panel.nckp\"", T::String),
                    ("message", T::Builtin),
                    ("\"synthetic fault\"", T::String),
                    ("end on", T::Keyword),
                    ("on note", T::Callback),
                    ("play_note", T::Builtin),
                ],
            ),
            fixture(
                Lang::Luau,
                LUAU,
                &[
                    ("require", Unsupported, LuaModule),
                    ("onNote", Implemented, Callback),
                    ("setParameter", Approximate, LuaParameter),
                    ("dsp.process", Stub, LuaApi),
                    ("print", Fault, LuaApi),
                ],
                &[
                    ("local", T::Keyword),
                    ("dsp", T::Variable),
                    ("require", T::Builtin),
                    ("\"SyntheticDSP\"", T::String),
                    ("function", T::Keyword),
                    ("onNote", T::Callback),
                    ("setParameter", T::Builtin),
                    ("\"Pitch\"", T::String),
                    ("0", T::Number),
                    ("dsp.process", T::Builtin),
                    ("print", T::Builtin),
                    ("\"synthetic fault\"", T::String),
                    ("end", T::Keyword),
                ],
            ),
        ]
    }
    pub fn state() -> State {
        let mut state = State::default();
        state.scripts = reports()
            .into_iter()
            .map(|(r, s)| Script::new(r, s))
            .collect();
        state
    }
    pub fn coverage(script: &Script) -> Coverage {
        Coverage {
            script_id: script.report.id.clone(),
            hits: script
                .report
                .constructs
                .iter()
                .filter(|c| c.span.line < 6)
                .map(|c| (c.span.start, 3))
                .collect(),
            faults: vec![(
                script
                    .report
                    .constructs
                    .iter()
                    .find(|c| c.status == Status::Fault)
                    .unwrap()
                    .span
                    .start,
                "Synthetic execution fault".into(),
            )],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::{Part, SamplerParams};
    use std::sync::Mutex;

    const SIZE: Size = Size::new(1180., 640.);
    fn tick(ui: &mut Ui, state: &mut State, input: Input) {
        let root = draw(ui, state, "scripts-test").w(SIZE.width).h(SIZE.height);
        ui.frame(root, Some(SIZE), input, 1. / 60.).unwrap();
    }
    fn press(ui: &mut Ui, state: &mut State, id: &str) {
        ui.focus(id);
        tick(
            ui,
            state,
            Input {
                keys: vec![KeyPress {
                    key: Key::Enter,
                    mods: Mods::default(),
                }],
                ..Default::default()
            },
        );
        tick(ui, state, Input::default());
        tick(ui, state, Input::default());
    }

    fn loaded_scripts() -> sampler_ir::Instrument {
        let xml = "<UVI4><Program><EventProcessors><ScriptProcessor Name='Lua'><script>require('helper')</script></ScriptProcessor></EventProcessors></Program></UVI4>";
        let mut instrument = sampler_uvi::translate(xml, std::path::Path::new("."))
            .unwrap()
            .instrument;
        instrument.behaviors.push(sampler_ir::Behavior {
            name: "KSP".into(),
            language: sampler_ir::Language::Ksp,
            source: "on init\nend on".into(),
            slot: Some(3),
            state: vec![],
            requires: vec![],
        });
        let mut files = sampler_uvi::script::Scripts::default();
        files.insert("helper.lua", "local helper = 1".into());
        instrument.uvi_script_sources.as_mut().unwrap().files = files.sources();
        instrument
    }

    #[test]
    fn loaded_instrument_seam_pairs_both_languages_and_required_modules() {
        let instrument = loaded_scripts();
        let reports = script_reports(&instrument);
        let copy = instrument.clone();
        let sources = instrument.uvi_script_sources.as_ref().unwrap();
        let copied = copy.uvi_script_sources.as_ref().unwrap();
        assert!(Arc::ptr_eq(&sources.xml, &copied.xml));
        assert!(Arc::ptr_eq(&sources.files, &copied.files));
        assert!(serde_json::to_value(&instrument).unwrap().get("uvi_script_sources").is_none());
        assert!(!format!("{sources:?}").contains("local helper"));
        let mut files = sampler_uvi::script::Scripts::from_sources(sources.files.clone());
        files.insert("other.lua", "".into());
        assert_eq!(sources.files.len(), 1);
        assert_eq!(files.sources().len(), 2);
        assert_eq!(reports.len(), 3);
        assert!(
            reports.iter().any(|(r, s)| r.id == "ksp:slot3"
                && r.lang == Lang::Ksp
                && &**s == "on init\nend on")
        );
        assert!(reports.iter().any(|(r, s)| r.id == "uvi:module:helper"
            && r.lang == Lang::Luau
            && &**s == "local helper = 1"));
        assert!(
            reports
                .iter()
                .any(|(r, s)| r.id.starts_with("uvi:/") && &**s == "require('helper')")
        );
        for (report, source) in reports {
            assert_eq!(report.source_len as usize, source.len());
            assert!(
                report
                    .tokens
                    .iter()
                    .all(|t| (t.span.end as usize) <= source.len())
            );
        }
    }

    #[test]
    fn coverage_toggle_never_reports_unobserved_live_execution_as_zero_hits() {
        let instrument = loaded_scripts();
        for (report, _) in script_reports(&instrument) {
            assert_eq!(script_coverage(&instrument, &report.id, true), None);
            assert_eq!(script_coverage(&instrument, &report.id, true), None);
            assert_eq!(script_coverage(&instrument, &report.id, false), None);
        }
        assert_eq!(script_coverage(&instrument, "missing", true), None);
    }

    #[test]
    fn fixture_reports_render_expected_gutters_and_inspect_in_out() {
        let p = Arc::new(SamplerParams::new());
        p.selection.write().unwrap().parts.push(Part {
            path: "/synthetic/ScriptInspector.nki".into(),
            ..Default::default()
        });
        {
            let mut v = p.shared.view.lock().unwrap();
            v.parts.resize_with(1, Default::default);
            v.parts[0].active = "Synthetic script inspector".into();
            v.parts[0].instrument = Some(Arc::new(sampler_ir::Instrument {
                name: "Synthetic script inspector".into(),
                ..Default::default()
            }));
        }
        let mut editor = super::super::tests::Harness::new(&p, 1180., 780.);
        editor.press("view-0-Scripts");
        for (line, status) in [
            (1, Status::Implemented),
            (2, Status::Implemented),
            (3, Status::Approximate),
            (4, Status::Stub),
            (5, Status::Unsupported),
            (6, Status::Fault),
        ] {
            assert!(
                editor
                    .ui
                    .scene()
                    .unwrap()
                    .surface(&format!("scripts-0-gutter-{line}-Some({status:?})"))
                    .is_some(),
                "expected status on line {line}"
            );
        }
        editor.press("scripts-0-live");
        editor.press("scripts-0-construct-2-3");
        assert!(
            editor
                .ui
                .scene()
                .unwrap()
                .surface("scripts-0-detail")
                .is_some()
        );
        #[cfg(feature = "shots")]
        if let Some(out) =
            std::env::var_os("KONTRA_SCRIPT_VIEW_SHOTS").map(std::path::PathBuf::from)
        {
            std::fs::create_dir_all(&out).unwrap();
            editor.idle(10);
            moose::core::screenshot::save_png(
                &out.join("scripts-ksp.png"),
                &super::super::tests::pixels(&editor.ui, 1180, 780),
                1180,
                780,
            );
            editor.press("scripts-0-script-1");
            editor.idle(10);
            moose::core::screenshot::save_png(
                &out.join("scripts-luau.png"),
                &super::super::tests::pixels(&editor.ui, 1180, 780),
                1180,
                780,
            );
        }
        editor.press("scripts-0-script-1");
        // The screenshot path may already have selected Luau; selection is idempotent.
        for (line, status) in [
            (1, Status::Unsupported),
            (2, Status::Implemented),
            (3, Status::Approximate),
            (4, Status::Stub),
            (5, Status::Fault),
        ] {
            assert!(
                editor
                    .ui
                    .scene()
                    .unwrap()
                    .surface(&format!("scripts-0-gutter-{line}-Some({status:?})"))
                    .is_some()
            );
        }
        let mut state = fixtures::state();
        let coverage = fixtures::coverage(&state.scripts[0]);
        state.scripts[0].observe(Some(coverage));
        let mut ui = super::super::theme::ui();
        for _ in 0..3 {
            tick(&mut ui, &mut state, Input::default());
        }
        assert_eq!(state.scripts[0].lines[2].hits, 3);
        assert_eq!(state.scripts[0].lines[8].hits, 0);
        assert_eq!(state.scripts[0].line_status(5), Some(Status::Fault));
        let instrument = Arc::new(sampler_ir::Instrument::default());
        state.instrument = Arc::downgrade(&instrument);
        press(&mut ui, &mut state, "scripts-test-live");
        assert!(state.collecting());
        press(&mut ui, &mut state, "scripts-test-live");
        assert!(!state.collecting());
        press(&mut ui, &mut state, "scripts-test-gaps");
        assert_eq!(state.matches, [2, 3, 4, 5]);
        press(&mut ui, &mut state, "scripts-test-next");
        assert_eq!(state.line, 3);
        press(&mut ui, &mut state, "scripts-test-status-Stub");
        assert_eq!(state.matches, [2, 4, 5]);
        press(&mut ui, &mut state, "scripts-test-construct-2-3");
        assert!(state.inspection.as_ref().unwrap().1[0].contains("fixture/host.rs:42"));
        assert!(state.inspection.as_ref().unwrap().1[1].contains("host.bind(set_engine_par)"));
        ui.focus("scripts-test-search");
        tick(
            &mut ui,
            &mut state,
            Input {
                text: "native".into(),
                ..Default::default()
            },
        );
        tick(&mut ui, &mut state, Input::default());
        assert_eq!(state.matches, [4]);
        let before = state.scripts[0].source.clone();
        ui.focus("scripts-test-code");
        tick(
            &mut ui,
            &mut state,
            Input {
                text: "must not edit".into(),
                ..Default::default()
            },
        );
        assert_eq!(state.scripts[0].source, before);
        let (mut report, source) = fixtures::reports().remove(0);
        report.constructs[2].status = Status::Implemented;
        let byte = report.constructs[2].span.start;
        let mut faulted = Script::new(report, source);
        let before = faulted.shares[Status::Fault as usize];
        faulted.observe(Some(Coverage {
            script_id: faulted.report.id.clone(),
            hits: vec![(byte, 1)],
            faults: vec![(byte, "Fresh host binding fault".into())],
        }));
        assert_eq!(faulted.construct_status(2), Status::Fault);
        assert_eq!(faulted.line_status(2), Some(Status::Fault));
        assert_eq!(faulted.shares[Status::Fault as usize], before + 1);
        assert!(construct_detail(&faulted, 2).contains("Runtime fault: Fresh host binding fault"));
        assert!(copy_gaps(&[faulted]).contains("**set_engine_par** [Fault]"));
        state.scripts[0].observe(Some(Coverage {
            script_id: "wrong-script".into(),
            ..Default::default()
        }));
        assert!(state.scripts[0].coverage.is_none());
    }

    #[test]
    fn ten_thousand_lines_stay_under_node_budget() {
        let source: Arc<str> = (1..=10_000)
            .map(|n| format!("message({n})"))
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        let mut report = ScriptReport {
            id: "ksp:large".into(),
            title: "10k synthetic lines".into(),
            lang: Lang::Ksp,
            source_len: source.len() as u32,
            tokens: vec![],
            constructs: vec![],
            lowered: vec![],
        };
        let mut start = 0;
        for (n, text) in source.split('\n').enumerate() {
            let span = Span {
                start,
                end: start + 7,
                line: n as u32 + 1,
            };
            report.tokens.push(sampler_parity::Token {
                span,
                class: TokenClass::Builtin,
            });
            report.constructs.push(sampler_parity::Construct {
                span,
                kind: ConstructKind::Builtin,
                name: "message".into(),
                status: STATUSES[n % STATUSES.len()],
                detail: "Synthetic 10k-line use".into(),
                owner: None,
                spec: None,
            });
            report.lowered.push(sampler_parity::Lowered {
                span,
                ops: vec![format!("host.message({})", n + 1)],
            });
            start += text.len() as u32 + 1;
        }
        let mut state = State::default();
        state.scripts = vec![Script::new(report, source)];
        let mut ui = super::super::theme::ui();
        ui.set_node_limit(600);
        for _ in 0..3 {
            tick(&mut ui, &mut state, Input::default());
        }
        assert!(ui.layout_stats().validated_nodes < 600);
        assert!(
            ui.scene()
                .unwrap()
                .surface("scripts-test-line-10000")
                .is_none()
        );
        let height = ui
            .scene()
            .unwrap()
            .surface("scripts-test-code")
            .unwrap()
            .frame
            .size
            .height;
        assert!(
            visible(state.matches.len(), state.y, height).len()
                <= (height / ROW).ceil() as usize + 1
        );
        ui.focus("scripts-test-code");
        tick(
            &mut ui,
            &mut state,
            Input {
                keys: vec![KeyPress {
                    key: Key::End,
                    mods: Mods::default(),
                }],
                ..Default::default()
            },
        );
        tick(&mut ui, &mut state, Input::default());
        assert!(
            ui.scene()
                .unwrap()
                .surface("scripts-test-line-10000")
                .is_some()
        );
        assert!(ui.scene().unwrap().surface("scripts-test-line-1").is_none());
        let start = std::time::Instant::now();
        for _ in 0..20 {
            tick(&mut ui, &mut state, Input::default());
        }
        eprintln!(
            "10k lines: {} validated nodes; debug steady build+frame mean {:.3} ms (contended, not acceptance)",
            ui.layout_stats().validated_nodes,
            start.elapsed().as_secs_f64() * 1000. / 20.
        );
        assert!(ui.layout_stats().validated_nodes < 600);
    }

    struct Board(Arc<Mutex<String>>);
    impl moose::mui::mui::Clipboard for Board {
        fn get(&mut self) -> Option<String> {
            Some(self.0.lock().unwrap().clone())
        }
        fn set(&mut self, text: &str) {
            *self.0.lock().unwrap() = text.into();
        }
    }
    #[test]
    fn copy_gaps_contains_source_lines_and_reaches_clipboard() {
        let clipboard = Arc::new(Mutex::new(String::new()));
        let mut ui = super::super::theme::ui().clipboard(Board(clipboard.clone()));
        let mut state = fixtures::state();
        for _ in 0..3 {
            tick(&mut ui, &mut state, Input::default());
        }
        press(&mut ui, &mut state, "scripts-test-copy");
        let copied = clipboard.lock().unwrap().clone();
        assert_eq!(copied, copy_gaps(&state.scripts));
        for expected in [
            "ksp:slot0 (KSP)",
            "uvi:/Program/EventProcessors/0 (Luau)",
            "L3 **set_engine_par** [Approximate]",
            "L4 **set_control_par** [Stub]",
            "L5 **load_native_ui** [Unsupported]",
            "L6 **message** [Fault]",
            "fixture/host.rs:42",
            "fixture/SCRIPT_SPEC.md#binding",
            "set_engine_par($ENGINE_PAR_VOLUME",
            "require(\"SyntheticDSP\")",
        ] {
            assert!(copied.contains(expected), "missing {expected}");
        }
        assert!(!copied.contains("**play_note**"));
        assert_eq!(markdown("a`b|c\nd"), "a\\`b\\|c d");
    }
}
