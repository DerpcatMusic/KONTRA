//! Every script control on a program's face, moved through the script UI
//! model (as the editor moves it, not by pixels) under a held note; then
//! what the load left out, and one JSON row per program.

use kontra_ui_model::{Kind, Value};
use moose::prelude::*;

use super::{BLOCK, In, Played, Report, at, block, cc, off, on};
use crate::{Dsp, KontraParams, Tick};

/// The most controls one program moves; past it the row says how many were left.
const MOST: usize = 300;
/// A control's move counts as a click when its loudest 5 ms of
/// high-frequency energy stands this far above the note before and after.
const SPIKE_DB: f32 = 15.0;

/// What touching a program's controls found.
#[derive(Default)]
pub(super) struct Touched {
    pub shown: usize,
    pub touched: usize,
    /// Controls whose move changed the held note's level by over 1 dB.
    pub reached: usize,
    pub clicks: Vec<String>,
    /// Controls whose move changed nothing audible (a menu that picks the
    /// next note's articulation is one; a dead knob is another).
    pub quiet: Vec<String>,
}

/// (widget, name, value to move it to) for every control that shows.
fn targets(params: &KontraParams) -> (usize, Vec<(usize, String, f64)>) {
    let rack = crate::lock(&params.shared.rack);
    let Some(view) = (rack.as_ref())
        .and_then(|r| r.parts().first()?.as_ref())
        .and_then(|p| p.view.as_ref())
    else {
        return (0, Vec::new());
    };
    let all: Vec<_> = (view.tree().widgets.iter().enumerate())
        .filter(|(_, w)| !w.hidden && w.enabled && w.source_id.is_some())
        .filter_map(|(n, w)| {
            let now = match w.value {
                Some(Value::Integer(i)) => f64::from(i),
                Some(Value::Real(r)) => r,
                _ => w.initial_value,
            };
            let to = match &w.kind {
                Kind::Knob { range, .. }
                | Kind::Slider { range, .. }
                | Kind::ValueEdit { range, .. } => {
                    let x = if now > (range.min + range.max) / 2.0 {
                        0.25
                    } else {
                        0.75
                    };
                    (range.min + x * (range.max - range.min)).round()
                }
                Kind::Switch | Kind::Button { momentary: false } => f64::from(u8::from(now < 0.5)),
                Kind::Menu { items } if items.len() > 1 => {
                    ((now.max(0.0) as usize + 1) % items.len()) as f64
                }
                _ => return None,
            };
            Some((n, w.name.clone(), to))
        })
        .collect();
    (all.len(), all.into_iter().take(MOST).collect())
}

/// The high-frequency energy of each 5 ms of `audio`.
fn hf_frames(audio: &[f32]) -> Vec<f32> {
    let d2: Vec<f32> = audio
        .windows(3)
        .map(|w| (w[2] - 2.0 * w[1] + w[0]).powi(2))
        .collect();
    d2.chunks_exact(at(0.005)).map(|c| c.iter().sum()).collect()
}

fn median(mut v: Vec<f32>) -> f32 {
    v.sort_by(f32::total_cmp);
    v.get(v.len() / 2).copied().unwrap_or(0.0)
}

fn rms_db(audio: &[f32]) -> f32 {
    let mean = audio.iter().map(|x| x * x).sum::<f32>() / audio.len().max(1) as f32;
    10.0 * mean.max(1e-20).log10()
}

/// Moves every control under a held C4, retriggered every 16 controls.
pub(super) fn touch(
    params: &KontraParams,
    dsp: &mut Dsp,
    last: &mut Played,
    fail: &mut dyn FnMut(String),
) -> Touched {
    let (shown, targets) = targets(params);
    let mut found = Touched {
        shown,
        ..Touched::default()
    };
    let mut run = |events: &[(usize, In)], seconds: f64, last: &mut Played| {
        let mut audio = Vec::new();
        let mut peak = 0.0f32;
        for b in 0..(at(seconds) / BLOCK).max(1) {
            let now: Vec<_> = if b == 0 { events.to_vec() } else { Vec::new() };
            *last = block(params, dsp, &now, last);
            audio.extend_from_slice(&last.audio);
            peak = last.channel_peaks.into_iter().fold(peak, f32::max);
        }
        Tick.run(params);
        (audio, peak)
    };
    let mut before = Vec::new();
    for (i, (widget, name, to)) in targets.into_iter().enumerate() {
        if i.is_multiple_of(16) {
            run(&[(0, off(60))], 0.2, last);
            before = run(&[(0, on(60)), (0, cc(1, 127)), (0, cc(11, 127))], 0.4, last).0;
        }
        if let Some(r) = crate::lock(&params.shared.rack).as_mut() {
            r.control(0, widget, None, to, Default::default());
        }
        let (after, channel_peak) = run(&[], 0.08, last);
        found.touched += 1;
        let tail = |a: &[f32]| {
            a.get(a.len().saturating_sub(at(0.03))..)
                .unwrap_or(&[])
                .to_vec()
        };
        let (was, now) = (tail(&before), tail(&after));
        let calm = median(hf_frames(&was)).max(median(hf_frames(&now)));
        let spike = hf_frames(&after).into_iter().fold(0.0f32, f32::max);
        let peak = after.iter().fold(0.0f32, |p, x| p.max(x.abs()));
        if channel_peak > 1.0 {
            fail(format!(
                "control {name}: clips at {:+.1} dBFS",
                20.0 * channel_peak.log10()
            ));
        }
        if peak > 1e-3 && 10.0 * (spike.max(1e-20) / calm.max(1e-20)).log10() > SPIKE_DB {
            found.clicks.push(name.clone());
        }
        if (rms_db(&now) - rms_db(&was)).abs() > 1.0 {
            found.reached += 1;
        } else {
            found.quiet.push(name);
        }
        before = after;
    }
    run(&[(0, off(60))], 0.2, last);
    if !found.clicks.is_empty() {
        fail(format!(
            "controls click when moved: {}",
            found.clicks.join(", ")
        ));
    }
    found
}

/// What the load could not play, by impact: (broken, audible, latent, the
/// most frequent broken and audible features).
pub(super) fn diagnostics(params: &KontraParams) -> (usize, usize, usize, Vec<String>) {
    let rack = crate::lock(&params.shared.rack);
    let Some(part) = rack.as_ref().and_then(|r| r.parts().first()?.as_ref()) else {
        return (0, 0, 0, Vec::new());
    };
    // By the impact's name: the plugin does not depend on the IR crate.
    let all: Vec<(String, &str)> = (part.diagnostics.iter())
        .map(|d| (format!("{:?}", d.impact), d.feature.as_str()))
        .collect();
    let count = |i: &str| all.iter().filter(|d| d.0 == i).count();
    let mut features: Vec<(String, &str, usize)> = Vec::new();
    for (impact, feature) in all
        .iter()
        .filter(|d| matches!(d.0.as_str(), "Broken" | "Audible"))
    {
        match features.iter_mut().find(|f| f.1 == *feature) {
            Some(f) => f.2 += 1,
            None => features.push((impact.clone(), feature, 1)),
        }
    }
    // Broken before Audible, then by how often.
    features.sort_by(|a, b| a.0.cmp(&b.0).then(b.2.cmp(&a.2)));
    let top = (features.iter().take(8))
        .map(|(i, f, n)| format!("{i} {f} ×{n}"))
        .collect();
    (count("Broken"), count("Audible"), count("Latent"), top)
}

fn json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out + "\""
}

fn list(items: &[String]) -> String {
    format!(
        "[{}]",
        items.iter().map(|s| json(s)).collect::<Vec<_>>().join(",")
    )
}

/// One program's row: it works, or exactly what is broken.
pub(super) fn row(path: &str, r: &Report) -> String {
    let (t, d) = (&r.ui, &r.diagnostics);
    let segments: Vec<String> = (r.segments.iter())
        .map(|s| {
            let [left, right] = s.channel_db;
            format!("{{\"segment\":{},\"voices\":{},\"stolen\":{},\"peak_db\":{:.1},\"peak_left_db\":{left:.1},\"peak_right_db\":{right:.1},\"hf_db\":{:.1}}}", json(s.name), s.voices, s.stolen, s.peak_db, s.hf_db)
        })
        .collect();
    format!(
        "{{\"program\":{},\"name\":{},\"works\":{},\"load_s\":{:.2},\"hold_ms\":{:.1},\"failed\":{},\"segments\":[{}],\
         \"ui\":{{\"shown\":{},\"touched\":{},\"reached\":{},\"clicks\":{},\"quiet\":{}}},\
         \"diagnostics\":{{\"broken\":{},\"audible\":{},\"latent\":{},\"top\":{}}}}}",
        json(path),
        json(&r.name),
        r.failed.is_empty() && d.0 == 0,
        r.load_s,
        r.hold_ms,
        list(&r.failed),
        segments.join(","),
        t.shown,
        t.touched,
        t.reached,
        list(&t.clicks),
        list(&t.quiet),
        d.0,
        d.1,
        d.2,
        list(&d.3),
    )
}
