//! Opt-in per-scenario frame cost for the editor. Run one scenario per process
//! (so `perf stat` can wrap it), in release:
//! `KONTRA_FRAME_SCENARIO=hover cargo test --release -p kontakto --lib frame_cost -- --ignored --nocapture`
//! Scenarios: idle (display-tick watch only), hover, drag, focus, meter, readout,
//! resize, hidpi2, hidpi3, tab, mixer (tab + live meters), sound (text-heavy tab),
//! keys (lit keyboard), typing, wheel, mixer16 (sixteen mixer strips, live meters).
//! `KONTRA_FRAME_COUNT` sets the frames per scenario (default 120). Prints medians in
//! ms: `build` (view closure), `layout` (`Ui::frame`, layout + scene), `encode` and
//! `raster_cpu` (vello CPU paint with a warm glyph/image cache, as the editor's software
//! path keeps one; the GPU path only pays `encode` on the CPU), plus the per-frame CPU
//! sum and % of one core at 60 fps.
use super::*;

#[test]
#[ignore = "timings only; select with KONTRA_FRAME_SCENARIO"]
fn frame_cost() {
    use moose::mui::mui::vello::{
        self,
        kurbo::Affine,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    use std::time::Instant;
    let scenario = std::env::var("KONTRA_FRAME_SCENARIO").unwrap_or_else(|_| "idle".into());
    let frames_wanted: usize = std::env::var("KONTRA_FRAME_COUNT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(120);
    let p = Arc::new(SamplerParams::new());
    p.shared.view.lock().unwrap().scanned = p.shared.libraries.wanted();
    let meters = Meters::default();
    let computer = computer::Computer::default();
    let mut h = tests::Harness::new(&p, 1180., 780.);
    let mut cache = vello::Cache::default();
    let mut resources = Resources::default();
    // One paint of the current scene at `scale`, as the editor's software path does it.
    // Returns (encode, raster) in ms: the GPU path pays the encode on the CPU too.
    let mut paint = |ui: &Ui, scale: f64| {
        let (w, hgt) = ((1180. * scale) as u16, (780. * scale) as u16);
        let mut ctx = RenderContext::new(w, hgt);
        let t = Instant::now();
        vello::paint(
            &mut vello::Cpu {
                ctx: &mut ctx,
                resources: &mut resources,
                cache: &mut cache,
            },
            ui.scene().unwrap(),
            Affine::scale(scale),
        )
        .unwrap();
        ctx.flush();
        let encode = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let mut pix = Pixmap::new(w, hgt);
        ctx.render(&mut pix, &mut resources);
        let raster = t.elapsed().as_secs_f64() * 1e3;
        (encode, raster)
    };
    let med = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(0.)
    };
    // Scenario setup: the tab the scenario lives on, shown before timing starts.
    if scenario == "mixer16" {
        // Sixteen loaded parts, so the mixer shows sixteen strips.
        p.shared.ensure_parts(16);
        let mut sel = write(&p.selection);
        sel.parts = (0..16)
            .map(|i| Part {
                path: format!("bench/strip{i}.nki").into(),
                ..Default::default()
            })
            .collect();
        sel.order = (0..16).collect();
    }
    match scenario.as_str() {
        "mixer" | "meter" | "mixer16" => h.press("tab-mixer"),
        "sound" => h.press("tab-sound"),
        _ => {}
    }
    h.idle(3);
    let (mut build, mut layout, mut paint_ms, mut encode_ms, mut watch_us) =
        (vec![], vec![], vec![], vec![], vec![]);
    // The CPU fallback's retained renderer: each frame repaints what changed
    // since the last one (`soft`), on the frame the scenario drew.
    let mut soft_ms = vec![];
    let mut verified = 0usize;
    let mut soft = moose::mui::mui::vello::software::Renderer::new((1180, 780)).unwrap();
    let mut watch = Watch::default();
    // Any stable on-screen control serves as the pointer target.
    let a = tests::center(&h.ui, "tab-rack");
    let b = Point::new(a.x + 40., a.y);
    let mut frames = 0usize;
    let mut timed_frame =
        |h: &mut tests::Harness, input: Input, build: &mut Vec<f64>, layout: &mut Vec<f64>| {
            let (b, l) = h.tick_split(input);
            build.push(b);
            layout.push(l);
            frames += 1;
        };
    for i in 0..frames_wanted {
        let mut painted = true;
        match scenario.as_str() {
            "idle" => {
                let t = Instant::now();
                let moved = watch.changed(&p, &meters, &computer);
                watch_us.push(t.elapsed().as_secs_f64() * 1e6);
                // Idle draws nothing: only frames the watch asks for are timed.
                if !moved {
                    continue;
                }
                timed_frame(&mut h, Input::default(), &mut build, &mut layout);
            }
            "hover" | "hidpi2" | "hidpi3" => {
                let pos = if i % 2 == 0 { a } else { b };
                let input = Input {
                    pointer: PointerInput {
                        pos: Some(pos),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                timed_frame(&mut h, input, &mut build, &mut layout);
            }
            "drag" => {
                let x = a.x + (i % 20) as f64;
                let input = Input {
                    pointer: PointerInput {
                        pos: Some(Point::new(x, a.y)),
                        buttons: Buttons::PRIMARY,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                timed_frame(&mut h, input, &mut build, &mut layout);
            }
            "meter" | "mixer" | "mixer16" => {
                // A live master meter: the watch runs frames on the animation clock.
                p.shared.meters.master[0].store(0.5f32.to_bits(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(16));
                if !watch.changed(&p, &meters, &computer) {
                    continue;
                }
                timed_frame(&mut h, Input::default(), &mut build, &mut layout);
            }
            "readout" => {
                p.shared.voices.store(i as u64 % 7, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(101));
                if !watch.changed(&p, &meters, &computer) {
                    continue;
                }
                timed_frame(&mut h, Input::default(), &mut build, &mut layout);
                painted = false;
            }
            "resize" => {
                // A window drag: the size changes every frame.
                let w = if i % 2 == 0 { 1180. } else { 1000. };
                h.resize(Size::new(w, 700.));
                timed_frame(&mut h, Input::default(), &mut build, &mut layout);
            }
            "tab" => {
                let tab = if i % 2 == 0 { "tab-mixer" } else { "tab-rack" };
                h.ui.focus(tab);
                timed_frame(&mut h, enter_key(), &mut build, &mut layout);
            }
            "sound" => {
                // Idle text-heavy panel: pointer moves over its rows.
                let pos = Point::new(a.x + (i % 30) as f64 * 3., a.y);
                let input = Input {
                    pointer: PointerInput {
                        pos: Some(pos),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                timed_frame(&mut h, input, &mut build, &mut layout);
            }
            "keys" => {
                // The keyboard lights one key per frame.
                p.shared.played[40 + i % 24].store(1 - (i as u8 % 2), Ordering::Relaxed);
                timed_frame(&mut h, Input::default(), &mut build, &mut layout);
            }
            "typing" => {
                h.ui.focus("search");
                let input = Input {
                    text: "k".into(),
                    ..Default::default()
                };
                timed_frame(&mut h, input, &mut build, &mut layout);
            }
            "wheel" => {
                let input = Input {
                    pointer: PointerInput {
                        pos: Some(a),
                        ..Default::default()
                    },
                    wheel: Vec2::new(0., if i % 2 == 0 { 12. } else { -12. }),
                    ..Default::default()
                };
                timed_frame(&mut h, input, &mut build, &mut layout);
            }
            "focus" => {
                h.ui.focus(if i % 2 == 0 { "search" } else { "tab-rack" });
                timed_frame(&mut h, Input::default(), &mut build, &mut layout);
            }
            other => panic!("unknown KONTRA_FRAME_SCENARIO {other}"),
        }
        if painted {
            let scale = match scenario.as_str() {
                "hidpi2" => 2.,
                "hidpi3" => 3.,
                _ => 1.,
            };
            let (enc, ras) = paint(&h.ui, scale);
            encode_ms.push(enc);
            paint_ms.push(enc + ras);
            let (w, hgt) = ((1180. * scale) as u32, (780. * scale) as u32);
            soft.resize((w, hgt)).unwrap();
            let t = Instant::now();
            soft.render(h.ui.scene().unwrap(), Affine::scale(scale))
                .unwrap();
            soft_ms.push(t.elapsed().as_secs_f64() * 1e3);
            // KONTRA_FRAME_VERIFY=1: every repainted frame must equal a full frame.
            if std::env::var_os("KONTRA_FRAME_VERIFY").is_some() {
                let mut full = moose::mui::mui::vello::software::Renderer::new((w, hgt)).unwrap();
                full.render(h.ui.scene().unwrap(), Affine::scale(scale))
                    .unwrap();
                if soft.pixels() != full.pixels() {
                    let wp = w as usize;
                    let bad: Vec<(usize, usize)> = soft
                        .pixels()
                        .chunks_exact(4)
                        .zip(full.pixels().chunks_exact(4))
                        .enumerate()
                        .filter(|(_, (a, b))| a != b)
                        .map(|(k, _)| (k % wp, k / wp))
                        .collect();
                    panic!(
                        "{scenario} frame {i}: damage repaint differs from a full frame in {} px, first {:?} last {:?}",
                        bad.len(),
                        bad.first(),
                        bad.last()
                    );
                }
                verified += 1;
            }
        }
    }
    let (b, l, pt, en, sf) = (
        med(&mut build),
        med(&mut layout),
        med(&mut paint_ms),
        med(&mut encode_ms),
        med(&mut soft_ms),
    );
    // Editor CPU per frame on the GPU path: build + layout + vello encode (raster is on the GPU).
    let total = b + l + en;
    println!(
        "FRAME_COST scenario={scenario} frames={frames} verified={verified} watch_us={:.2} build_ms={b:.3} layout_ms={l:.3} encode_ms={en:.3} raster_cpu_ms={:.3} soft_ms={sf:.3} cpu_ms={total:.3} cpu_at_60fps={:.1}%",
        med(&mut watch_us),
        pt - en,
        total * 60. / 10.
    );
}

fn enter_key() -> Input {
    Input {
        keys: vec![KeyPress {
            key: Key::Enter,
            mods: Mods::default(),
        }],
        ..Default::default()
    }
}
