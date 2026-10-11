//! Opt-in frame-cost bench for an installed UVI bank's Original editor.
//! `KONTRA_UVI_BANK` (a .ufs) and optionally `KONTRA_UVI_PROGRAM` (default: the
//! first program); medians over warm frames, in ms.
use super::*;

#[test]
#[ignore = "installed UVI bank required; timings only"]
fn uvi_frame_cost() {
    use std::time::Instant;
    let path = std::env::var("KONTRA_UVI_BANK").unwrap();
    let bank = sampler_uvi::Bank::open(Path::new(&path)).unwrap();
    let program = std::env::var("KONTRA_UVI_PROGRAM")
        .unwrap_or_else(|_| bank.programs().into_iter().next().unwrap());
    let (xml, _) = bank.program(&program).unwrap();
    let host = sampler_uvi::script::ScriptHost::new(&xml, bank.scripts(), sampler_uvi::script::Config::realtime()).unwrap();
    let face = host.interface();
    let p = Arc::new(SamplerParams::new());
    p.shared.ensure_parts(1);
    let values = host.control_values();
    let cells: Vec<_> = values
        .iter()
        .map(|(id, value)| crate::plugin::ControlCell::loop_audit_new(*id, *value))
        .collect();
    *p.shared.part(0).unwrap().controls.lock().unwrap() = cells.into();
    p.selection.write().unwrap().parts.push(crate::plugin::Part {
        path: Path::new(&path).join(&program).to_string_lossy().into_owned(),
        view: 1,
        ..Default::default()
    });
    println!("UVI_SCENARIO program={program} widgets={}", face.widgets.len());
    p.shared.view.lock().unwrap().parts[0].interfaces = vec![face].into();
    let mut h = tests::Harness::new(&p, 1180., 780.);
    h.ui.focus("tab-rack");
    let start = Instant::now();
    let mut revision = pictures::revision();
    let mut settled = Instant::now();
    while start.elapsed() < Duration::from_secs(90) {
        h.idle(1);
        let now = pictures::revision();
        if now != revision {
            revision = now;
            settled = Instant::now();
        }
        if settled.elapsed() > Duration::from_secs(2) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let med = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    let (mut tick, mut paint) = (vec![], vec![]);
    for _ in 0..100 {
        let t = Instant::now();
        h.idle(1);
        tick.push(t.elapsed().as_secs_f64() * 1000.);
        let t = Instant::now();
        let _ = tests::pixels(&h.ui, 1180, 780);
        paint.push(t.elapsed().as_secs_f64() * 1000.);
    }
    println!("UVI_FRAME_MS steady tick={:.2} sw_paint={:.2}", med(&mut tick), med(&mut paint));
}
