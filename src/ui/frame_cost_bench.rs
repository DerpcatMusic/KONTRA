//! Opt-in per-scenario frame cost for the editor. Run one scenario per process
//! (so `perf stat` can wrap it):
//! `KONTRA_FRAME_SCENARIO=hover cargo test -p kontakto --lib frame_cost -- --ignored --nocapture`
//! Scenarios: idle (display-tick watch only), hover, drag, readout, focus.
//! Prints medians in ms per frame: build+layout (`tick`) and CPU paint (`pixels`).
use super::*;

#[test]
#[ignore = "timings only; select with KONTRA_FRAME_SCENARIO"]
fn frame_cost() {
    use std::time::Instant;
    let scenario = std::env::var("KONTRA_FRAME_SCENARIO").unwrap_or_else(|_| "idle".into());
    let p = Arc::new(SamplerParams::new());
    p.shared.view.lock().unwrap().scanned = p.shared.libraries.wanted();
    let meters = Meters::default();
    let computer = computer::Computer::default();
    let mut h = tests::Harness::new(&p, 1180., 780.);
    h.idle(3);
    let med = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(0.)
    };
    let (mut tick, mut paint, mut watch_us) = (vec![], vec![], vec![]);
    let mut watch = Watch::default();
    // Any stable on-screen control serves as the pointer target.
    let a = tests::center(&h.ui, "tab-rack");
    let b = Point::new(a.x + 40., a.y);
    let mut frames = 0usize;
    for i in 0..120 {
        match scenario.as_str() {
            "idle" => {
                let t = Instant::now();
                let moved = watch.changed(&p, &meters, &computer);
                watch_us.push(t.elapsed().as_secs_f64() * 1e6);
                if moved {
                    frames += 1;
                }
                // Idle draws nothing: only frames the watch asks for are timed.
                if !moved {
                    continue;
                }
            }
            "hover" => {
                let pos = if i % 2 == 0 { a } else { b };
                let input = Input {
                    pointer: PointerInput { pos: Some(pos), ..Default::default() },
                    ..Default::default()
                };
                let t = Instant::now();
                h.tick(input);
                tick.push(t.elapsed().as_secs_f64() * 1e3);
                frames += 1;
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
                let t = Instant::now();
                h.tick(input);
                tick.push(t.elapsed().as_secs_f64() * 1e3);
                frames += 1;
            }
            "meter" => {
                // A live master meter: the watch runs frames on the animation clock.
                p.shared.meters.master[0].store(0.5f32.to_bits(), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(16));
                let t = Instant::now();
                if watch.changed(&p, &meters, &computer) {
                    h.tick(Input::default());
                    tick.push(t.elapsed().as_secs_f64() * 1e3);
                    frames += 1;
                    let _ = tests::pixels(&h.ui, 1180, 780);
                }
            }
            "readout" => {
                p.shared.voices.store(i as u64 % 7, Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(101));
                let t = Instant::now();
                if watch.changed(&p, &meters, &computer) {
                    h.tick(Input::default());
                    tick.push(t.elapsed().as_secs_f64() * 1e3);
                    frames += 1;
                }
            }
            _ => {
                let t = Instant::now();
                h.ui.focus(if i % 2 == 0 { "search" } else { "tab-rack" });
                h.tick(Input::default());
                tick.push(t.elapsed().as_secs_f64() * 1e3);
                frames += 1;
            }
        }
        if !matches!(scenario.as_str(), "idle" | "readout" | "meter") {
            let t = Instant::now();
            let _ = tests::pixels(&h.ui, 1180, 780);
            paint.push(t.elapsed().as_secs_f64() * 1e3);
        }
    }
    if scenario == "meter" {
        println!("FRAME_COST scenario=meter frames_per_s={:.1} tick_ms={:.3} paint_ms={:.3}", frames as f64 / 2.0, med(&mut tick), 0.);
        return;
    }
    println!(
        "FRAME_COST scenario={scenario} frames={frames} watch_us={:.2} tick_ms={:.3} paint_ms={:.3}",
        med(&mut watch_us),
        med(&mut tick),
        med(&mut paint)
    );
}
