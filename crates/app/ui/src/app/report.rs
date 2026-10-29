//! The report: what did not load or play as the source meant. One line per
//! entry, its summary; a click opens the whole of it.

use mui::prelude::*;

use super::{Action, App};
use crate::kit::{self, Icon, button, field, glyph, interactive, strip};
use crate::theme::{CONTROL, SPACE, TEXT, TIGHT, secondary};

pub(super) fn view(ui: &mut Ui, app: &mut App, out: &mut Vec<Action>) -> El {
    let mut tools = Vec::new();
    for (id, label, action) in [
        ("report-copy", "Copy diagnostics", Action::CopyDiagnostics),
        ("report-export", "Export…", Action::ExportReport),
        ("report-logs", "Log folder", Action::LogFolder),
        ("report-refresh", "Refresh", Action::Refresh),
    ] {
        let b = button(ui, id, label, false);
        if b.changed {
            out.push(action);
        }
        tools.push(b.el);
    }
    let v = &mut app.view;
    let search = field(
        ui,
        "report-search",
        "Filter the report",
        &mut v.report_search,
    );
    let needle = v.report_search.to_lowercase();
    let shown: Vec<(usize, &String)> = app
        .report
        .iter()
        .enumerate()
        .filter(|(_, l)| needle.is_empty() || l.to_lowercase().contains(&needle))
        .collect();
    let summary = caption(format!(
        "{} matching · {} retained",
        shown.len(),
        app.report.len()
    ))
    .fill(secondary());
    let mut rows = Vec::new();
    let mut toggled = None;
    for &(n, entry) in &shown {
        let id = format!("report-{n}");
        if ui.get(id.clone()).activated() {
            toggled = Some(n);
        }
        let open = v.report_open == Some(n);
        let first = entry.lines().next().unwrap_or_default().to_owned();
        let mut lines = vec![
            row![
                glyph(
                    if open { Icon::Down } else { Icon::Right },
                    TEXT,
                    secondary()
                ),
                glyph(Icon::Warning, TEXT, secondary()),
                body(first).lines(1).min_w(0).grow(1).tip(entry.clone()),
            ]
            .gap(SPACE)
            .align(Align::Center)
            .h(CONTROL),
        ];
        let rest: Vec<&str> = entry.lines().skip(1).collect();
        if open && !rest.is_empty() {
            lines.push(
                row![
                    block(TEXT * 3., 1).shrink(0),
                    caption(rest.join("\n")).fill(secondary()).grow(1).min_w(0)
                ]
                .w(Len::Pct(100.))
                .pad(kit::edges(0., SPACE, SPACE, 0.)),
            );
        }
        rows.push(
            interactive(col(lines).w(Len::Pct(100.)).pad((0., SPACE)))
                .focusable()
                .a11y(A11y::Button)
                .id(id),
        );
    }
    if let Some(n) = toggled {
        v.report_open = (v.report_open != Some(n)).then_some(n);
    }
    let list = if app.report.is_empty() {
        caption("Everything loaded as written.")
            .fill(secondary())
            .pad(SPACE * 2.)
    } else {
        col(rows).gap(1).w(Len::Pct(100.)).scroll()
    };
    col![
        strip(tools),
        row![search.el.w(TEXT * 24.), summary]
            .gap(SPACE)
            .align(Align::Center)
            .pad((SPACE, TIGHT)),
        kit::rule(),
        list.min_h(0).grow(1),
    ]
    .w(Len::Pct(100.))
}
