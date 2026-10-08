//! Gainer kernel against its equation (DSP_SYSTEM_INVENTORY "Gainer" recurrence,
//! KONTAKT_REFERENCE s.25: `(1-m) + m*g`, native float32 k = 1/1800):
//! out = x * (dry + cur), cur += (target - cur) * k, across odd block sizes.
use sampler_core::*;
mod support;
const LEVEL: ControlId = ControlId(9);
const RATE: u32 = 48000;
const DRY: f64 = 0.5;
const FRAMES: usize = 400;

fn limits() -> Limits {
    Limits {
        notes: 4, channels: 0, performances: 1, families: 4, expressions: 4, voices: 4,
        decisions: 0, commands: 4, behaviors: 0, behavior_fuel: 0, behavior_cells: 0, note_cells: 0,
    }
}

fn plan() -> Prepared {
    Prepared::new(
        RATE,
        vec![Pcm::new(RATE, vec![[1.; 2]; 4096].into_boxed_slice()).unwrap()],
        vec![Region {
            sample: 0, key_low: 60, key_high: 60, root_key: None, velocity_low: 0.,
            velocity_high: 1., gain: 1., envelope: Envelope::default(), playback: Playback::default(),
        }],
        1,
    )
    .unwrap()
    .with_controls(vec![ControlDefinition {
        id: LEVEL,
        domain: ControlDomain::Real { min: 0., max: 1. },
        default: ControlValue::Real(1.),
    }])
    .unwrap()
    .with_voice_chains(
        vec![VoiceChain::new(
            vec![],
            vec![Processor::Gainer {
                dry: DRY,
                gain: Parameter::Control(ControlRange { control: LEVEL, low: 0., high: 1., ramp_frames: 0 }),
            }],
            0,
        )
        .unwrap()],
        vec![Some(0)],
    )
    .unwrap()
}

#[test]
fn gain_steps_follow_a_one_pole_around_the_dry_mix() {
    let k = f32::from_bits(0x3a11a2b4);
    for block in [1, 7, 64, 129] {
        let mut rt = Runtime::new(plan(), limits()).unwrap();
        let mut audio = vec![[0f32; 2]; FRAMES];
        support::without_heap(|| {
            let p = rt.active_plan();
            let input = Input { protocol: Protocol::Clap, port: 0, group: 0, channel: 0, key: 60, external_id: Some(1) };
            rt.trigger(input, 60, 1.).unwrap();
            let mut first = [[0f32; 2]; 1];
            rt.render(&mut first).unwrap();
            assert!((f64::from(first[0][0]) - (DRY + 1.)).abs() < 1e-6, "starts at its first target");
            rt.edit_controls(p, None, &[ControlWrite { id: LEVEL, value: ControlValue::Real(0.) }]).unwrap();
            for chunk in audio.chunks_mut(block) {
                rt.render(chunk).unwrap();
            }
        });
        let mut current = 1f32;
        for (j, f) in audio.iter().enumerate() {
            let expected = DRY + f64::from(current);
            assert!((f64::from(f[0]) - expected).abs() < 2e-6, "block {block} frame {j}: {} != {expected}", f[0]);
            current += (0. - current) * k;
        }
    }
}
