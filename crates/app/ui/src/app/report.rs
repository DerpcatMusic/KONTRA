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
    let dev_label = if app.report_latent.is_empty() {
        "Developer".to_owned()
    } else {
        format!("Developer ({})", app.report_latent.len())
    };
    let dev = button(
        ui,
        "report-developer",
        &dev_label,
        app.view.report_developer,
    );
    if dev.changed {
        app.view.report_developer = !app.view.report_developer;
        app.view.report_open = None;
    }
    tools.push(dev.el.tip("Show latent developer diagnostics"));
    for (n, entry) in app.report.iter().enumerate() {
        if entry.ends_with(", use it?") {
            let b = button(
                ui,
                format!("report-relink-{n}"),
                "Use the found folder",
                false,
            );
            if b.changed {
                out.push(Action::Relink(entry.clone()));
            }
            tools.push(b.el);
        }
    }
    let v = &mut app.view;
    let search = field(
        ui,
        "report-search",
        "Filter the report",
        &mut v.report_search,
    );
    let needle = v.report_search.to_lowercase();
    let entries: Vec<&String> = if v.report_developer {
        app.report.iter().chain(app.report_latent.iter()).collect()
    } else {
        app.report.iter().collect()
    };
    let shown: Vec<(usize, &String)> = entries
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, l)| needle.is_empty() || l.to_lowercase().contains(&needle))
        .collect();
    let summary_text = if !v.report_developer && !app.report_latent.is_empty() {
        format!(
            "{} matching · {} retained · {} developer hidden",
            shown.len(),
            entries.len(),
            app.report_latent.len()
        )
    } else {
        format!("{} matching · {} retained", shown.len(), entries.len())
    };
    let summary = caption(summary_text).fill(secondary());
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
                body(first.clone())
                    .lines(1)
                    .min_w(0)
                    .grow(1)
                    .tip(entry.clone()),
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
                .named(first)
                .id(id),
        );
    }
    if let Some(n) = toggled {
        v.report_open = (v.report_open != Some(n)).then_some(n);
    }
    let list = if entries.is_empty() {
        let empty = if app.report_latent.is_empty() {
            "Everything loaded as written."
        } else {
            "Everything loaded as written. Enable Developer to inspect latent details."
        };
        caption(empty).fill(secondary()).pad(SPACE * 2.)
    } else {
        col(rows)
            .gap(1)
            .w(Len::Pct(100.))
            .scroll()
            .id("report-list")
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
