use kontra_dsp_filter::{Clocked, ClockedShape};

/// Kontakt 8's Phaser filter (type 13) and Vowel A/B (14, 15), their native
/// clock (0x140ae9f90), setters and kernels run under kemu
/// (tools/re/phaser_kemu_vectors.py, vowel_kemu_vectors.py): per case the
/// host calls (cutoff goal, resonance goal, frames), the input and the
/// native output. The allpasses are held at their 48 kHz corner, so only
/// their 48 kHz cases are here.
#[test]
fn clocked_filters_match_the_native_code() {
    let text = include_str!("clocked_vectors.txt");
    let mut lines = text.lines();
    let floats = |l: &str| -> Vec<f32> { l.split(' ').filter_map(|v| v.parse().ok()).collect() };
    let mut cases = 0;
    while let Some(head) = lines.next() {
        let [name, class, rate] = head.split(' ').collect::<Vec<_>>()[..] else {
            panic!("bad head {head}");
        };
        let shape = match class {
            "a" => ClockedShape::VowelLow,
            "b" => ClockedShape::VowelHigh,
            _ => ClockedShape::Allpass,
        };
        let calls: Vec<Vec<f32>> = lines
            .next()
            .unwrap()
            .split(' ')
            .map(|c| c.split(',').filter_map(|v| v.parse().ok()).collect())
            .collect();
        let stim = floats(lines.next().unwrap());
        let native = floats(lines.next().unwrap());
        let mut f = Clocked::new(shape, [calls[0][0], calls[0][1]], rate.parse().unwrap());
        let mut io: Vec<[f32; 2]> = stim.iter().map(|&x| [x, x]).collect();
        let mut rest = &mut io[..];
        for c in &calls {
            let (chunk, tail) = rest.split_at_mut(c[2] as usize);
            f.set([c[0], c[1]]);
            f.process(chunk);
            rest = tail;
        }
        let peak = native.iter().fold(1e-6f32, |m, v| m.max(v.abs()));
        let worst = io
            .iter()
            .zip(&native)
            .map(|(got, want)| (got[0] - want).abs().max((got[1] - want).abs()) / peak)
            .fold(0.0, f32::max);
        assert!(worst < 1e-5, "{name}: {worst:e} of peak");
        cases += 1;
    }
    assert_eq!(cases, 18);
}
