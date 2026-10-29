//! A part's mapping, as v2 showed it: its groups with a filter, its zones
//! on a key × velocity grid in their group's colour, the zones under the
//! cell last picked, and the picked zone's root, tune, volume and pan. A
//! pick auditions; picking the same cell again cycles the zones under it.

use mui::prelude::*;

use super::rack::pan_text;
use super::{Action, Part, View, Zone};
use crate::keyboard::note_name;
use crate::kit::{self, button, field, list_row, section};
use crate::theme::{SPACE, TEXT, TIGHT, hairline, lift, part_color, secondary};

pub(super) fn view(ui: &mut Ui, p: &Part, n: usize, v: &mut View, out: &mut Vec<Action>) -> El {
    if p.zones.is_empty() {
        return col![
            section("Mapping"),
            body("This instrument maps no zones.").fill(secondary())
        ]
        .gap(SPACE)
        .align(Align::Start)
        .pad(SPACE);
    }
    let shown: Vec<usize> = (0..p.zones.len())
        .filter(|&z| {
            v.mapping_group
                .is_none_or(|g| p.zones.get(z).is_some_and(|z| z.group == g))
        })
        .collect();
    let groups = groups(ui, p, n, v);
    let grid = grid(ui, p, n, &shown, v, out);
    let under = under(ui, p, n, v);
    let detail = detail(ui, p, n, v, out);
    col![
        row![groups, col![grid, under].gap(SPACE).grow(1).min_w(0)]
            .gap(SPACE)
            .align(Align::Start),
        detail
    ]
    .gap(SPACE)
    .w(Len::Pct(100.))
}

fn groups(ui: &mut Ui, p: &Part, n: usize, v: &mut View) -> El {
    let search = field(
        ui,
        format!("part-{n}-group-search"),
        "Filter groups",
        &mut v.mapping_search,
    );
    let all = list_row(
        ui,
        format!("part-{n}-group-all"),
        ("All groups", ""),
        None,
        v.mapping_group.is_none(),
    );
    if all.changed {
        v.mapping_group = None;
    }
    let needle = v.mapping_search.to_lowercase();
    let mut rows = vec![search.el, all.el];
    for (g, name) in p.groups.iter().enumerate() {
        if !needle.is_empty() && !name.to_lowercase().contains(&needle) {
            continue;
        }
        let count = p.zones.iter().filter(|z| z.group == g).count().to_string();
        let r = list_row(
            ui,
            format!("part-{n}-group-{g}"),
            (name, &count),
            Some(part_color(g)),
            v.mapping_group == Some(g),
        );
        if r.changed {
            v.mapping_group = Some(g);
        }
        rows.push(r.el);
    }
    col(rows)
        .gap(1)
        .w(TEXT * 13.)
        .h(TEXT * 22.)
        .shrink(0)
        .scroll()
}

/// The keys the grid spans: the shown zones', whole octaves.
fn span(zones: &[&Zone]) -> (u8, u8) {
    let lo = zones.iter().map(|z| z.keys.0).min().unwrap_or(36);
    let hi = zones.iter().map(|z| z.keys.1).max().unwrap_or(96);
    (lo - lo % 12, (hi - hi % 12 + 11).min(127))
}

/// The zones of `zones` (by index into the part's) that hold `cell`.
fn holding(p: &Part, zones: &[usize], (key, vel): (u8, u8)) -> Vec<usize> {
    zones
        .iter()
        .copied()
        .filter(|&z| {
            p.zones.get(z).is_some_and(|z| {
                (z.keys.0..=z.keys.1).contains(&key)
                    && (z.velocities.0..=z.velocities.1).contains(&vel)
            })
        })
        .collect()
}

fn grid(
    ui: &mut Ui,
    p: &Part,
    n: usize,
    shown: &[usize],
    v: &mut View,
    out: &mut Vec<Action>,
) -> El {
    let id = format!("part-{n}-grid");
    let zones: Vec<&Zone> = shown.iter().filter_map(|&z| p.zones.get(z)).collect();
    let (lo, hi) = span(&zones);
    let keys = f64::from(hi - lo + 1);
    let height = TEXT * 16.;
    let size = ui
        .scene()
        .and_then(|s| s.surface(&id))
        .map(|s| s.frame.size);
    if ui.get(id.clone()).clicked
        && let Some((at, size)) = ui.local(id.clone()).zip(size)
    {
        let key = lo + ((at.x / size.width * keys).floor().clamp(0., keys - 1.)) as u8;
        let vel = (127. - (at.y / size.height * 127.).floor()).clamp(1., 127.) as u8;
        let hits = holding(p, shown, (key, vel));
        // The same cell again: the next zone under it.
        let next = match (v.mapping_cell == Some((key, vel)), v.mapping_zone) {
            (true, Some(z)) => hits.iter().position(|&h| h == z).map_or(0, |i| i + 1),
            _ => 0,
        };
        v.mapping_cell = Some((key, vel));
        v.mapping_zone = hits.get(next % hits.len().max(1)).copied();
        if let Some(zone) = v.mapping_zone {
            out.push(Action::Audition { part: n, zone });
        }
    }
    let cells: Vec<(f64, f64, f64, f64, Color, bool)> = shown
        .iter()
        .filter_map(|&z| {
            let zone = p.zones.get(z)?;
            Some((
                f64::from(zone.keys.0 - lo) / keys,
                f64::from(zone.keys.1 - lo + 1) / keys,
                1. - f64::from(zone.velocities.1) / 127.,
                1. - f64::from(zone.velocities.0.saturating_sub(1)) / 127.,
                part_color(zone.group),
                v.mapping_zone == Some(z),
            ))
        })
        .collect();
    let octaves = usize::from((hi - lo + 1) / 12);
    let canvas = canvas(move |s| {
        let rect = |x0: f64, y0: f64, x1: f64, y1: f64| {
            Path::polyline(
                [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map(|(x, y)| Point::new(x, y)),
                true,
            )
        };
        let mut draw = vec![Draw::fill(rect(0., 0., s.width, s.height), lift(1))];
        for o in 1..octaves {
            let x = s.width * o as f64 / octaves as f64;
            draw.push(Draw::fill(rect(x, 0., x + 1., s.height), hairline()));
        }
        // The picked zone last, so its outline is on top.
        let order = cells
            .iter()
            .filter(|c| !c.5)
            .chain(cells.iter().filter(|c| c.5));
        for &(x0, x1, y0, y1, c, picked) in order {
            let r = rect(x0 * s.width, y0 * s.height, x1 * s.width, y1 * s.height);
            draw.push(Draw::fill(r.clone(), Fill::from(c.with_alpha(0.35))));
            let edge = if picked {
                Fill::from(Role::Ink)
            } else {
                Fill::from(c)
            };
            draw.push(Draw::stroke(r, edge, if picked { 2. } else { 1. }));
        }
        draw
    })
    .w(Len::Pct(100.))
    .h(height)
    .cursor(Cursor::Hand)
    .tracks_pointer()
    .named("Zones")
    .tip("Click a cell to audition · again to cycle the zones under it")
    .id(id);
    let groups = p.groups.len();
    col![
        row![
            caption(format!("{} zones · {groups} groups", p.zones.len())).fill(secondary()),
            spacer(),
            caption(format!("{}–{}", note_name(lo), note_name(hi))).fill(secondary()),
        ],
        row![
            col![
                caption("127").fill(secondary()),
                spacer(),
                caption("1").fill(secondary())
            ]
            .h(height)
            .align(Align::End),
            canvas,
        ]
        .gap(TIGHT),
    ]
    .gap(TIGHT)
    .w(Len::Pct(100.))
}

fn under(ui: &mut Ui, p: &Part, n: usize, v: &mut View) -> El {
    let Some(cell) = v.mapping_cell else {
        return spacer().h(0);
    };
    let all: Vec<usize> = (0..p.zones.len()).collect();
    let hits = holding(p, &all, cell);
    let mut rows = vec![section(&format!(
        "Zones under {} · velocity {}",
        note_name(cell.0),
        cell.1
    ))];
    for z in hits {
        let Some(zone) = p.zones.get(z) else { continue };
        let at = format!(
            "{}–{} / {}–{}",
            note_name(zone.keys.0),
            note_name(zone.keys.1),
            zone.velocities.0,
            zone.velocities.1
        );
        let r = list_row(
            ui,
            format!("part-{n}-zone-{z}"),
            (&zone.sample, &at),
            Some(part_color(zone.group)),
            v.mapping_zone == Some(z),
        );
        if r.changed {
            v.mapping_zone = Some(z);
        }
        rows.push(r.el);
    }
    col(rows).gap(1).w(Len::Pct(100.))
}

fn detail(ui: &mut Ui, p: &Part, n: usize, v: &View, out: &mut Vec<Action>) -> El {
    let Some((z, zone)) = v.mapping_zone.and_then(|z| Some((z, p.zones.get(z)?))) else {
        return caption("Pick a zone on the grid to see it here.").fill(secondary());
    };
    let play = button(ui, format!("part-{n}-audition"), "Audition", false);
    if play.changed {
        out.push(Action::Audition { part: n, zone: z });
    }
    let line = format!(
        "Root {} · Tune {:+.0} ct · Volume {:+.1} dB · Pan {}",
        note_name(zone.root),
        zone.tune,
        zone.volume,
        pan_text(zone.pan)
    );
    kit::strip(vec![
        col![
            body(zone.sample.clone())
                .text_weight(Weight::SEMIBOLD)
                .lines(1)
                .min_w(0)
                .tip(zone.sample.clone()),
            caption(line).fill(secondary()),
        ]
        .gap(1)
        .align(Align::Start)
        .grow(1)
        .min_w(0),
        play.el,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_spans_whole_octaves_and_finds_the_zones_under_a_cell() {
        let zone = |group, keys, velocities| Zone {
            group,
            keys,
            velocities,
            ..Zone::default()
        };
        let p = Part {
            zones: vec![
                zone(0, (40, 50), (1, 64)),
                zone(1, (40, 50), (65, 127)),
                zone(1, (45, 70), (1, 127)),
            ],
            ..Part::default()
        };
        let all: Vec<&Zone> = p.zones.iter().collect();
        assert_eq!(span(&all), (36, 71));
        assert_eq!(holding(&p, &[0, 1, 2], (46, 30)), [0, 2]);
        assert_eq!(holding(&p, &[0, 1, 2], (46, 100)), [1, 2]);
        assert_eq!(holding(&p, &[1], (46, 30)), Vec::<usize>::new());
    }
}
