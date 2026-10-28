use kontra_dsp_filter::{Formant, VowelSet, Vowels};

/// Kontakt 8's Formant I/II magnitude at 48 kHz (Wine oracle,
/// tools/kontakt-oracle/eqrender.py, impulse against the bypass reference):
/// talk, sharp, size, second set, then dB at `PROBES`.
#[rustfmt::skip]
const KONTAKT: [(f32, f32, f32, bool, [f64; 10]); 17] = [
    (0.25,0.5,0.5, false, [-23.28, -17.08, -12.20, -3.70, 5.18, -10.38, -9.24, -17.06, -24.96, -30.90]),
    (0.5,0.5,0.5, false, [-27.01, -20.95, -16.44, -10.06, 1.35, -2.15, -0.77, -5.56, -13.75, -19.73]),
    (0.75,0.5,0.5, false, [-24.93, -18.81, -14.14, -6.89, 3.53, -8.48, 13.38, -2.83, -11.75, -17.83]),
    (1.0,0.5,0.5, false, [-25.16, -18.99, -14.20, -6.22, -2.66, -18.52, 1.45, 10.12, -8.22, -14.88]),
    (0.5,0.0,0.5, false, [-25.28, -19.22, -14.71, -8.34, 2.71, 2.73, -0.09, -4.55, -12.59, -18.55]),
    (0.5,1.0,0.5, false, [-28.00, -21.94, -17.43, -11.04, 0.44, -5.85, 0.01, -5.99, -14.26, -20.25]),
    (0.5,0.5,0.0, false, [-20.95, -14.79, -10.05, -2.30, -2.11, -0.67, -2.32, -12.66, -19.99, -25.83]),
    (0.5,0.5,1.0, false, [-33.10, -27.07, -22.61, -16.50, -7.89, 1.29, -2.33, 8.24, -6.75, -13.37]),
    (0.0,0.5,0.5, true, [-25.29, -19.00, -13.88, -2.97, -7.56, -7.88, 7.19, -14.72, -22.62, -28.56]),
    (0.25,0.5,0.5, true, [-22.65, -16.10, -10.09, -9.88, -3.49, 0.80, 11.45, -6.63, -14.69, -20.66]),
    (0.5,0.5,0.5, true, [-26.75, -20.69, -16.19, -9.83, 1.83, 10.22, 4.13, -1.83, -10.32, -16.34]),
    (0.75,0.5,0.5, true, [-27.91, -21.86, -17.36, -11.05, 0.03, -6.39, 5.06, -1.43, -9.91, -15.93]),
    (1.0,0.5,0.5, true, [-26.09, -19.33, -12.72, -7.43, -18.80, -7.82, 9.76, -2.86, -12.69, -18.89]),
    (0.5,0.0,0.5, true, [-24.95, -18.89, -14.39, -8.05, 2.97, 9.81, 3.56, -1.03, -9.35, -15.35]),
    (0.5,1.0,0.5, true, [-27.79, -21.74, -17.23, -10.88, 0.94, 9.25, 4.98, -2.10, -10.67, -16.71]),
    (0.5,0.5,0.0, true, [-20.68, -14.54, -9.81, -2.08, 10.24, 4.22, 1.71, -9.22, -16.61, -22.47]),
    (0.5,0.5,1.0, true, [-32.86, -26.83, -22.37, -16.27, -7.71, 1.76, 10.13, 13.37, -3.04, -9.89]),
];
const PROBES: [f64; 10] = [
    30., 60., 100., 200., 500., 1000., 2000., 5000., 10000., 15000.,
];

#[test]
fn the_formant_matches_kontakt_within_a_tenth_of_a_db() {
    const RATE: u32 = 48_000;
    for (talk, sharp, size, second, want) in KONTAKT {
        let v = Vowels {
            talk,
            sharp,
            size,
            set: if second {
                VowelSet::Second
            } else {
                VowelSet::First
            },
        };
        let mut f = Formant::new(v, RATE);
        let mut ir = vec![[0.0f32; 2]; 16_384];
        ir[0] = [1.0, 1.0];
        f.process(&mut ir);
        for (hz, want) in PROBES.iter().zip(want) {
            let (mut re, mut im) = (0.0f64, 0.0f64);
            for (n, x) in ir.iter().enumerate() {
                let phase = std::f64::consts::TAU * hz * n as f64 / f64::from(RATE);
                re += f64::from(x[0]) * phase.cos();
                im -= f64::from(x[0]) * phase.sin();
            }
            let got = 10.0 * (re * re + im * im).log10();
            assert!(
                (got - want).abs() < 0.1,
                "{v:?} at {hz} Hz: {got:.2} dB, Kontakt {want}"
            );
        }
    }
}

#[test]
fn a_band_past_nyquist_stays_stable() {
    // talk 1, size 1 doubles the top band to 9.5 kHz, past 8 kHz Nyquist.
    let v = Vowels {
        talk: 1.0,
        sharp: 0.5,
        size: 1.0,
        set: VowelSet::First,
    };
    let mut f = Formant::new(v, 16_000);
    let mut io = vec![[0.0f32; 2]; 32_000];
    io[0] = [1.0, 1.0];
    f.process(&mut io);
    let tail = io[16_000..].iter().map(|x| x[0].abs()).fold(0.0, f32::max);
    assert!(tail < 1e-3, "tail {tail}");
}

#[test]
fn a_knob_change_glides_over_15_ms_then_holds_the_target() {
    const RATE: u32 = 48_000;
    let v = |talk| Vowels {
        talk,
        sharp: 0.5,
        size: 0.5,
        set: VowelSet::First,
    };
    let sine = |n: usize| {
        let x = (std::f64::consts::TAU * 1000.0 * n as f64 / f64::from(RATE)).sin() as f32;
        [x, x]
    };
    let peak = |f: &mut Formant, from: usize, to: usize| {
        let mut io: Vec<[f32; 2]> = (from..to).map(sine).collect();
        f.process(&mut io);
        io.iter().map(|x| x[0].abs()).fold(0.0, f32::max)
    };
    let mut target = Formant::new(v(1.0), RATE);
    peak(&mut target, 0, 4800);
    let settled = peak(&mut target, 4800, 9600);
    let mut moving = Formant::new(v(0.0), RATE);
    peak(&mut moving, 0, 4800);
    moving.set(v(1.0), RATE);
    // 23 ticks of 32 frames: 736 frames, then the filter rings out the old
    // state.
    let early = peak(&mut moving, 4800, 4900);
    peak(&mut moving, 4900, 4800 + 736 + 2400);
    let late = peak(&mut moving, 4800 + 736 + 2400, 9600);
    assert!((early / settled - 1.0).abs() > 0.1, "{early} {settled}");
    assert!((late / settled - 1.0).abs() < 1e-3, "{late} {settled}");
}
