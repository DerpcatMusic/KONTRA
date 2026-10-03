//! Original authored bank exercised through the production loader and callback.
use super::*;
use crate::uvi::{
    host::{UiEdit, UiEditValue, UiModifiers, UiValue},
    playback::Renderer,
    program::parse_program,
    script::{Action, Command, Note},
    worker::Stamp,
};
use moose::core::ProcessMode;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const BLOCK: usize = 256;
const PROGRAM: &str = r#"<Program Gain="1"><EventProcessors><ScriptProcessor><script><![CDATA[
function onInit()
    setSize(180,100)
    makePerformanceView()
    level=Knob{name='authored level',value=1,min=0,max=1,bounds={10,10,60,60}}
    level.changed=function(self)Program:setParameter('Gain',self.value)end
end
function onNote(e)postEvent(e)end
function onRelease(e)postEvent(e)end
]]></script></ScriptProcessor></EventProcessors><Layers><Layer><Keygroups><Keygroup>
<Oscillators><MinBlepGenerator Waveform="4" StartPhase="0.25" BaseNote="60"/></Oscillators>
</Keygroup></Keygroups></Layer></Layers></Program>"#;

fn block(dsp: &mut Dsp, params: &SamplerParams) -> [[f32; BLOCK]; 2] {
    let mut audio = [[0.; BLOCK]; 2];
    let [left, right] = &mut audio;
    let mut channels = [left.as_mut_slice(), right.as_mut_slice()];
    let mut buffer = AudioBuffer::from_slices_checked(&[], &mut channels, BLOCK);
    let events = EventList::with_capacity(0);
    let mut output = EventList::with_capacity(128);
    let transport = TransportInfo::default();
    let mut cx = ProcessContext::new(&transport, params.shared.rate(), BLOCK, &mut output)
        .with_process_mode(ProcessMode::Offline);
    assert_eq!(
        tests::allocations(|| {
            Sampler::process(dsp, params, &mut buffer, &events, &mut cx);
        }),
        0,
        "native adoption, keyboard input and offline playback must not allocate on the callback"
    );
    audio
}

fn render(dsp: &mut Dsp, params: &SamplerParams, frames: usize) -> Vec<[f32; 2]> {
    assert_eq!(frames % BLOCK, 0);
    let mut audio = Vec::with_capacity(frames);
    for _ in 0..frames / BLOCK {
        let piece = block(dsp, params);
        audio.extend((0..BLOCK).map(|frame| [piece[0][frame], piece[1][frame]]));
        Load.run(params);
    }
    audio
}

fn wait_live(dsp: &mut Dsp, params: &SamplerParams) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        Load.run(params);
        let live = {
            let view = params.shared.view.lock().unwrap();
            (0..2).all(|slot| {
                let part = &view.parts[slot];
                part.uvi_activation.as_ref().is_some_and(|activation| {
                    params
                        .shared
                        .part(slot)
                        .unwrap()
                        .uvi_generation
                        .load(Ordering::Acquire)
                        == activation.generation
                }) && !part.loading
                    && part.uvi_ui.as_ref().is_some_and(|ui| {
                        ui.snapshots.iter().any(|snapshot| {
                            snapshot.root.performance_view && !snapshot.widgets.is_empty()
                        })
                    })
            })
        };
        if live {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "authored native slots did not become live"
        );
        block(dsp, params);
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn edit(params: &SamplerParams, slot: usize, value: f64) -> (Stamp, UiEdit) {
    let published = params.shared.view.lock().unwrap().parts[slot]
        .uvi_ui
        .clone()
        .unwrap();
    let (processor, widget) = published
        .snapshots
        .iter()
        .find_map(|snapshot| {
            snapshot
                .widgets
                .iter()
                .find(|widget| widget.name == "authored level")
                .map(|widget| (snapshot.processor, widget.id))
        })
        .unwrap();
    (
        published.stamp,
        UiEdit {
            processor,
            widget,
            value: UiEditValue::Number(value),
            modifiers: UiModifiers::default(),
        },
    )
}

#[test]
fn restored_native_rack_adopts_after_delay_ack_and_renders_keyboard_gain_pan_and_ui() {
    let directory = std::env::temp_dir().join(format!(
        "kontra-uvi-plugin-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let bank = directory.join("authored.ufs");
    crate::library::tests::authored_uvi_bank_with_source(&bank, 9, PROGRAM.as_bytes());
    let mut params = SamplerParams::new();
    params.shared.libraries = crate::library::tests::authored_uvi_scanner(&directory);
    let source = library::UviSource {
        bank,
        bank_uuid: [9; 16],
        member: "Piano.uvip".into(),
    };
    let part = Part {
        uvi: Some(source.clone()),
        gain: -20.,
        pan: 0.5,
        ..Default::default()
    };
    // Exercise persisted identity, including two slots using the same bank/member.
    let restored: Part = serde_json::from_str(&serde_json::to_string(&part).unwrap()).unwrap();
    *params.selection.write().unwrap() = Selection {
        parts: vec![restored, Part { mute: true, ..part }],
        order: vec![0, 1],
        ..Default::default()
    };
    let mut dsp = Dsp::default();
    Sampler::reset(&mut dsp, &params, &AudioConfig::new(48000., BLOCK));
    let zone = crate::import::Zone {
        loop_range: Some(crate::import::Loop {
            start: 0,
            end: 64,
            alternating: false,
            until_release: false,
            crossfade: 0,
        }),
        ..Default::default()
    };
    dsp.rack.parts[0].set_bank(Some(Box::new(
        Bank::from_samples(
            vec![crate::import::Group::default()],
            vec![zone],
            vec![(
                PathBuf::new(),
                crate::audio::Sample {
                    rate: 48000,
                    frames: vec![[0.2; 2]; 64],
                },
            )],
        )
        .unwrap(),
    )));
    dsp.rack.parts[0].note_on(0, 60, 100);
    Load.run(&params);
    let initial = params.shared.view.lock().unwrap().parts[0]
        .uvi_activation
        .clone()
        .unwrap();
    assert!(!initial.published);
    assert!(params.shared.view.lock().unwrap().parts[0].loading);
    assert_eq!(
        params
            .shared
            .part(0)
            .unwrap()
            .uvi_generation
            .load(Ordering::Acquire),
        0
    );
    assert_eq!(
        dsp.rack.parts[0].active_voices(),
        1,
        "preparation must retain the current player"
    );
    let part_generation = initial.part_generation;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        block(&mut dsp, &params); // callback first acknowledges shared delay storage
        Load.run(&params);
        let activation = params.shared.view.lock().unwrap().parts[0]
            .uvi_activation
            .clone()
            .unwrap();
        assert_eq!(
            activation.part_generation, part_generation,
            "duplicate loader polls restarted the slot"
        );
        if activation.published {
            assert!(
                params.shared.view.lock().unwrap().parts[0].loading,
                "queued handoff must remain preparing until callback installation"
            );
            assert_eq!(
                params
                    .shared
                    .part(0)
                    .unwrap()
                    .uvi_generation
                    .load(Ordering::Acquire),
                0
            );
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    wait_live(&mut dsp, &params);
    assert!(
        dsp.rack.parts[0].bank().is_none(),
        "callback adopted the actual native endpoint"
    );
    assert!(dsp.uvi[0].is_some() && dsp.uvi[1].is_some());
    let (stamp, _) = edit(&params, 0, 1.);
    let (other_stamp, _) = edit(&params, 1, 1.);
    assert_ne!(
        stamp.generation, other_stamp.generation,
        "same program in two slots needs separate controllers"
    );
    let latency = dsp.uvi[0].as_ref().unwrap().latency_frames() as usize;
    assert_eq!(latency, (UVI_LEAD_PACKETS + 1) * 256);
    params.shared.press_key(0, 60, 100);
    let audio = render(&mut dsp, &params, latency + 1024);
    assert!(audio[..latency].iter().all(|frame| *frame == [0.; 2]));
    let program = parse_program(PROGRAM).unwrap();
    let mut reference = Renderer::new(&program, Default::default(), 48000).unwrap();
    let expected = reference
        .render(
            &[Command {
                frame: 0,
                action: Action::Start(Note {
                    id: 1,
                    note: 60,
                    velocity: 100,
                    channel: 0,
                    dim1: 0,
                    dim2: None,
                    layers: None,
                    oscillator: None,
                    volume: 1.,
                    pan: 0.,
                    tune: 0.,
                    offset_us: 0,
                }),
            }],
            &[],
            1024,
        )
        .unwrap();
    let master = db_to_linear(params.volume.read());
    let gain = db_gain(-20.);
    let pan = 0.5;
    let expected: Vec<_> = expected
        .iter()
        .map(|frame| {
            [
                frame[0] * (gain * (1. - pan)) * master,
                frame[1] * gain * master,
            ]
        })
        .collect();
    assert!(expected.iter().any(|frame| frame[0].abs() > 0.001));
    let mut first_mismatch = None;
    let mut max_error = 0_f32;
    for (index, (actual, expected)) in audio[latency..].iter().zip(&expected).enumerate() {
        for channel in 0..2 {
            max_error = max_error.max((actual[channel] - expected[channel]).abs());
            if first_mismatch.is_none() && actual[channel].to_bits() != expected[channel].to_bits()
            {
                first_mismatch = Some((
                    index,
                    channel,
                    actual[channel].to_bits(),
                    expected[channel].to_bits(),
                ));
            }
        }
    }
    assert!(
        first_mismatch.is_none(),
        "bank worker PCM must reach the gain/pan mixer exactly once; first mismatch {first_mismatch:?}, maximum error {max_error}"
    );
    assert!(
        !params
            .shared
            .part(0)
            .unwrap()
            .uvi_failed
            .load(Ordering::Acquire)
    );

    let (stamp, change) = edit(&params, 0, 0.);
    assert!(
        !params.shared.edit_uvi(1, stamp, change),
        "same widget in another slot must reject this generation"
    );
    assert!(!params.shared.edit_uvi(
        0,
        Stamp {
            generation: stamp.generation + 1,
            ..stamp
        },
        change
    ));
    assert!(params.shared.edit_uvi(0, stamp, change));
    let muted = render(&mut dsp, &params, latency + 1024);
    assert!(
        muted[muted.len() - 512..]
            .iter()
            .all(|frame| *frame == [0.; 2]),
        "the native changed callback did not mute Program gain"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let observed = params.shared.view.lock().unwrap().parts[0]
            .uvi_ui
            .as_ref()
            .is_some_and(|ui| {
                ui.snapshots
                    .iter()
                    .flat_map(|snapshot| &snapshot.widgets)
                    .any(|widget| {
                        widget.name == "authored level" && widget.value == Some(UiValue::Number(0.))
                    })
            });
        if observed {
            break;
        }
        assert!(Instant::now() < deadline);
        block(&mut dsp, &params);
        Load.run(&params);
        std::thread::sleep(Duration::from_millis(1));
    }
    let (stamp, restore) = edit(&params, 0, 1.);
    assert!(params.shared.edit_uvi(0, stamp, restore));
    assert!(
        render(&mut dsp, &params, latency + 1024)
            .iter()
            .rev()
            .take(512)
            .any(|frame| frame[0].abs() > 0.001)
    );
    params.shared.release_key(60);
    let released = render(&mut dsp, &params, latency + 1024);
    assert!(
        released
            .iter()
            .rev()
            .take(512)
            .all(|frame| *frame == [0.; 2]),
        "keyboard release must reach the native source"
    );

    // Persisted identities survive host reset while stale widgets and old error
    // flags must not control or falsely fail the new epoch/rate/block context.
    Sampler::reset(&mut dsp, &params, &AudioConfig::new(44100., BLOCK * 2));
    params
        .shared
        .part(0)
        .unwrap()
        .uvi_failed
        .store(true, Ordering::Release);
    Load.run(&params);
    let next = params.shared.view.lock().unwrap().parts[0]
        .uvi_activation
        .clone()
        .unwrap();
    assert_ne!(
        (next.epoch, next.generation),
        (initial.epoch, initial.generation)
    );
    assert_eq!((next.rate, next.max_host_frames), (44100, BLOCK * 2));
    assert!(params.shared.view.lock().unwrap().parts[0].loading);
    assert!(!params.shared.edit_uvi(0, stamp, restore));
    wait_live(&mut dsp, &params);
    assert!(
        !params
            .shared
            .part(0)
            .unwrap()
            .uvi_failed
            .load(Ordering::Acquire)
    );
    let current = params.shared.view.lock().unwrap().parts[0]
        .uvi_activation
        .clone()
        .unwrap();
    params.selection.write().unwrap().parts[0].uvi = None;
    Load.run(&params);
    block(&mut dsp, &params);
    Load.run(&params);
    assert!(
        params.shared.view.lock().unwrap().parts[0]
            .uvi_activation
            .is_none()
    );
    assert!(params.shared.view.lock().unwrap().parts[0].uvi_ui.is_none());
    assert!(dsp.uvi[0].is_none());
    assert!(
        params
            .shared
            .uvi_controls
            .lock()
            .unwrap()
            .status(current.epoch, current.generation)
            .is_none()
    );
    params.selection.write().unwrap().parts[1].uvi = None;
    Load.run(&params);
    block(&mut dsp, &params);
    Load.run(&params);
    assert_eq!(dsp.uvi_latency, 0);
    assert_eq!(
        Sampler::latency(&dsp),
        dsp.align.plan.latency(dsp.rack.parts[0].rate())
    );
    assert!(params.shared.uvi_controls.lock().unwrap().shutdown());
    drop(dsp);
    drop(params);
    std::fs::remove_dir_all(directory).unwrap();
}
