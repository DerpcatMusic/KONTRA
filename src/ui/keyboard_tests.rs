//! Piano painting and range-row receipts, with synthetic publication only.
use super::*;
use tests::{Harness, pixels};

fn specimen(multi: bool) -> Arc<SamplerParams> {
    let p = Arc::new(SamplerParams::new());
    let ranges = if multi { vec![(36, 48), (60, 72), (84, 96)] } else { vec![(36, 72)] };
    p.selection.write().unwrap().parts = ranges.iter().enumerate().map(|(n, _)| Part {
        path: format!("/virtual/Piano {n}.nki"), name: format!("Piano {n}"), collapsed: true, ..Default::default()
    }).collect();
    let mut v = p.shared.view.lock().unwrap();
    for (n, &(low, high)) in ranges.iter().enumerate() {
        let part = &mut v.parts[n];
        part.active = format!("Piano {n}");
        let mut report = crate::sound::report::LoadReport::default();
        report.decoded.keys = crate::sound::report::range_bits(low, high);
        part.report = Some(Arc::new(report));
        let mut keys = vec![crate::sound::KeyLook::default(); 128];
        for key in &mut keys { key.color = Some(10); }
        for key in &mut keys[64..=72] { key.color = Some(6); }
        keys[24] = crate::sound::KeyLook { color: Some(0), control: true, ..Default::default() };
        part.keys = keys.into();
    }
    drop(v);
    p
}

fn sample(pixels: &[u8], width: usize, x: f64, y: f64) -> [u8; 3] {
    let at = (y as usize * width + x as usize) * 4;
    pixels[at..at+3].try_into().unwrap()
}

#[test]
fn authored_range_colors_preserve_piano_faces_and_unmapped_grey() {
    for (width, height) in [(900, 600), (1180, 900)] {
        let p = specimen(false);
        let h = Harness::new(&p, width as f64, height as f64);
        let image = pixels(&h.ui, width, height);
        let face = |note| {
            let f = h.ui.scene().unwrap().surface(&format!("key-{note}")).unwrap().frame;
            sample(&image, width as usize, f.x + f.size.width * 0.75, f.y + f.size.height * 0.8)
        };
        let white = face(60); let black = face(61); let grey = face(12); let control = face(24);
        assert!(white.iter().map(|&c| u32::from(c)).sum::<u32>() > black.iter().map(|&c| u32::from(c)).sum::<u32>() + 300,
            "authored range colours keep white keys light and black keys dark: {white:?}/{black:?}");
        assert!(grey.into_iter().max().unwrap() - grey.into_iter().min().unwrap() < 3,
            "a coloured range does not turn unmapped keys into controls: {grey:?}");
        assert!(control[0] > control[1] * 2, "explicit control keys keep their authored colour: {control:?}");
    }
}

#[test]
fn every_instrument_gets_its_own_range_row_even_without_overlap() {
    for (width, height) in [(900., 600.), (1180., 900.)] {
        let p = specimen(true);
        let h = Harness::new(&p, width, height);
        let scene = h.ui.scene().unwrap();
        let ranges = scene.surface("part-ranges").unwrap();
        assert_eq!(ranges.frame.size.height, 11., "three instruments need three 3px rows with 1px gaps");
        assert_eq!(ranges.tip.as_deref().unwrap().lines().count(), 3);
        let keys = scene.surface("keys").unwrap().frame;
        assert!(ranges.frame.y + ranges.frame.size.height <= keys.y, "range rows stay above the keys");
        assert!(keys.y + keys.size.height <= height, "the dock remains inside the window");
    }
}

#[test]
#[cfg(feature = "shots")]
fn keyboard_p0_shots() {
    let Some(out) = std::env::var_os("KONTRA_KEYBOARD_SHOTS").map(PathBuf::from) else { return };
    std::fs::create_dir_all(&out).unwrap();
    for (width, height) in [(900, 600), (1180, 900)] {
        for multi in [false, true] {
            let p = specimen(multi);
            let mut h = Harness::new(&p, width as f64, height as f64);
            h.idle(20);
            moose::core::screenshot::save_png(&out.join(format!("{}-{width}.png", if multi {"multi"} else {"single"})),
                &pixels(&h.ui, width, height), width.into(), height.into());
        }
    }
}
