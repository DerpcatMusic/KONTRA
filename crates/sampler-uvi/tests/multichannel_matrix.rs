//! Authored normal-loader causal contract; no bank/native/source inspection.
//! Static cases use one oscillator, linear stages and an explicit stored loop.
//! They establish lost inputs and signed matrix orientation, not native wide-
//! graph reduction, bypass width, FXPostGain envelope or oscillator grouping.
use sampler_core::{ControlValue, Input, Limits, NoteId, Protocol, Runtime};
use sampler_uvi::{script::ScriptHost, scripted::{Driver, HostInput}};
use std::{path::{Path, PathBuf}, sync::atomic::{AtomicU64, Ordering}};

const RATE: u32 = 48_000;
const SOURCE_FRAMES: usize = 2_048;
const NOTE_FRAMES: usize = 8_192;
const BEFORE_WRITE: usize = 4_096;
const MAX_BLOCK_FRAMES: usize = 127;
const STREAM_WAIT: std::time::Duration = std::time::Duration::from_secs(2);
const VELOCITY: f64 = 0.73;
// Coefficients in exact integer quarters: matrix[output][input].
type Matrix = [[i16; 6]; 2];
static NEXT: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        // A writable scratch directory: the runner's temporary directory when
        // it is set, otherwise the workspace scratch, otherwise the platform
        // temporary directory. A hard-coded workspace path is not writable on
        // a CI runner.
        let scratch = std::env::var_os("TMPDIR").map(PathBuf::from).unwrap_or_else(|| {
            let local = PathBuf::from("/mnt/Windows11/DEV_WORKSPACE/BuildScratch");
            if std::fs::create_dir_all(&local).is_ok() { local } else { std::env::temp_dir() }
        });
        std::fs::create_dir_all(&scratch).unwrap();
        let path = scratch.join(format!("uvi-matrix-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn program<const N: usize>(&self, name: &str, samples: &[[i16; N]], matrix: Matrix, script: &str) -> PathBuf {
        assert!(matches!(N, 1 | 2 | 6));
        assert_eq!(samples.len(), SOURCE_FRAMES);
        std::fs::write(self.0.join(format!("{name}.wav")), wav(samples)).unwrap();
        let code = if script.is_empty() { String::new() } else {
            format!("<EventProcessors><ScriptProcessor><script><![CDATA[{script}]]></script></ScriptProcessor></EventProcessors>")
        };
        // All comparisons use exactly these same authored factors. No inferred
        // absolute engine gain is used to derive the reference PCM contract.
        let xml = format!(r#"<Program Gain="0.75"><Layers><Layer Gain="0.625"><Keygroups>
<Keygroup LowKey="60" HighKey="60" Gain="0.5" FXPostGain="0">
<Inserts>{matrix}</Inserts>
<ControlSignalSources><DAHDSR Name="Amp" AttackTime="0.003" DecayTime="0.002" SustainLevel="0.7" ReleaseTime="0.002"/></ControlSignalSources>
<Connections><SignalConnection Source="$Keygroup/Amp" Destination="Gain" Ratio="1"/></Connections>
<Oscillators><SamplePlayer SamplePath="{name}.wav" BaseNote="60" FineTune="0" Gain="0.875">
<PlaybackOptions Start="0" Stop="{SOURCE_FRAMES}"><Loop Start="0" End="{SOURCE_FRAMES}" Type="0"/></PlaybackOptions>
</SamplePlayer></Oscillators></Keygroup></Keygroups></Layer></Layers>{code}</Program>"#, matrix=matrix_xml(matrix));
        let path = self.0.join(format!("{name}.uvip"));
        std::fs::write(&path, xml).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
}

fn wav<const N: usize>(samples: &[[i16; N]]) -> Vec<u8> {
    let channels = u16::try_from(N).unwrap();
    let bytes = u32::try_from(samples.len() * N * 2).unwrap();
    let mut out = b"RIFF".to_vec(); out.extend((36 + bytes).to_le_bytes());
    out.extend(b"WAVEfmt "); out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes()); out.extend(channels.to_le_bytes());
    out.extend(RATE.to_le_bytes()); out.extend((RATE * u32::from(channels) * 2).to_le_bytes());
    out.extend((channels * 2).to_le_bytes()); out.extend(16u16.to_le_bytes());
    out.extend(b"data"); out.extend(bytes.to_le_bytes());
    for frame in samples { for value in frame { out.extend(value.to_le_bytes()); } }
    out
}
fn tone(frame: usize, harmonic: usize, phase: usize) -> i16 {
    // Integral quarter-scaled samples make signed reference sums exactly
    // representable by PCM16, without quantization or a clipping policy.
    let phase = ((frame * harmonic + phase) % 64) as f64 * std::f64::consts::TAU / 64.;
    4 * (phase.sin() * 512.).round() as i16
}
fn wide(first_two: bool, upper: bool) -> Vec<[i16; 6]> {
    (0..SOURCE_FRAMES).map(|i| [
        if first_two { tone(i, 5, 3) } else { 0 },
        if first_two { tone(i, 7, 11) } else { 0 },
        if upper { tone(i, 1, 0) } else { 0 },
        if upper { tone(i, 2, 5) } else { 0 },
        if upper { tone(i, 3, 9) } else { 0 },
        if upper { tone(i, 4, 15) } else { 0 },
    ]).collect()
}
fn identity() -> Matrix { let mut m = [[0; 6]; 2]; m[0][0] = 4; m[1][1] = 4; m }
fn matrix_xml(matrix: Matrix) -> String {
    let mut out = String::from("<GainMatrix");
    for input in 0..12 { for output in 0..12 {
        let q = if input < 6 && output < 2 { matrix[output][input] } else { 0 };
        assert!((-4..=4).contains(&q));
        // Public Gain_i_j is input i -> output j, including signed inversion.
        out += &format!(" Gain_{}_{}=\"{}\"", input + 1, output + 1, f64::from(q) / 4.);
    } }
    out + "/>"
}
fn stereo_reference(samples: &[[i16; 6]], matrix: Matrix) -> Vec<[i16; 2]> {
    samples.iter().map(|frame| std::array::from_fn(|output| {
        let signed_sum: i32 = frame.iter().zip(matrix[output]).map(|(&sample, q)| i32::from(sample) * i32::from(q)).sum();
        assert_eq!(signed_sum % 4, 0, "authored oracle must not round");
        i16::try_from(signed_sum / 4).expect("authored matrix reference must not clip")
    })).collect()
}
fn input() -> Input { Input { protocol: Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None } }

struct Session {
    rt: Runtime,
    ir: sampler_ir::Instrument,
    note: NoteId,
    _stream: Option<(sampler_kontakt::Streamer, Vec<sampler_core::Pcm>)>,
    stream_horizon: Option<u32>,
}
impl Session {
    fn load(path: &Path, streamed: bool) -> Self {
        let (loaded, cache, keep, stream_horizon) = if streamed {
            let s = sampler_uvi::load_streamed(path, RATE, &Default::default()).unwrap();
            // Normal streamer contract: heads cover starts, running voices need
            // block service ahead of them. This changes no source/loop geometry.
            let horizon = s.report.head_frames.max(sampler_core::PAGE_FRAMES)
                .checked_add(MAX_BLOCK_FRAMES).unwrap();
            assert!(horizon <= 2 * RATE as usize, "unexpected authored-test stream horizon");
            let horizon = u32::try_from(horizon).unwrap();
            (s.loaded, Some(s.cache), Some((s.streamer, s.assets)), Some(horizon))
        } else { (sampler_uvi::load(path, RATE).unwrap(), None, None, None) };
        let limits = Limits::for_plan(&loaded.plan, 4, 4);
        let mut rt = Runtime::new(loaded.plan, limits).unwrap();
        if let Some(cache) = cache {
            rt = rt.with_stream_cache(cache);
            rt.set_stream_horizon(stream_horizon.unwrap()).unwrap();
            // This fixture runs entirely off audio. Use the ordinary offline
            // segment readiness after due controls, as well as boundary service.
            rt.set_offline(true);
        }
        let note = rt.trigger(input(), 60, VELOCITY).unwrap();
        Self { rt, ir: loaded.instrument, note, _stream: keep, stream_horizon }
    }
    fn ready(&mut self, frames: usize) {
        if let Some(horizon) = self.stream_horizon {
            let horizon = horizon.max(u32::try_from(frames).unwrap());
            let now = self.rt.now();
            // wait_streaming calls ordinary service_streaming: poll bounded
            // completions, protect/request exact demand and wake the worker.
            // It must return complete readiness or a real error/timeout. Never
            // render missing pages, mute a failure or move the note clock here.
            self.rt.wait_streaming(horizon, STREAM_WAIT).unwrap();
            assert_eq!(self.rt.now(), now, "storage readiness must not advance the note clock");
        }
    }
    fn render(&mut self, frames: usize) -> Vec<[f32; 2]> {
        assert!(frames <= NOTE_FRAMES + BEFORE_WRITE);
        let mut out = vec![[0.; 2]; frames];
        // Exercise normal internal segmentation; no source cursor hooks.
        let blocks = [31, MAX_BLOCK_FRAMES, 64, 13]; let mut at = 0; let mut block = 0;
        while at < frames {
            let end = (at + blocks[block % blocks.len()]).min(frames);
            self.ready(end - at);
            self.rt.render(&mut out[at..end]).unwrap();
            at = end; block += 1;
        }
        self.healthy();
        assert_eq!(self.rt.voice_count(), 1, "one held oscillator remains one physical voice");
        out
    }
    fn healthy(&mut self) {
        let s = self.rt.stats();
        assert_eq!(s.nonfinite_frames, 0); assert_eq!(s.stream_underruns, 0);
        assert_eq!(s.voice_drops, 0); assert_eq!(s.voice_pool_refusals, 0);
        assert_eq!(s.refused_starts, 0); assert_eq!(s.cold_starts, 0);
        assert!(self.rt.take_stream_fault().is_none());
    }
    fn close(mut self) {
        self.rt.release(self.note).unwrap(); self.ready(512);
        self.rt.render(&mut [[0.; 2]; 512]).unwrap();
        self.rt.flush_ended(|_| true); self.healthy();
        assert_eq!(self.rt.voice_count(), 0, "one release retires the same source");
    }
}
fn render(path: &Path, streamed: bool, frames: usize) -> Vec<[f32; 2]> {
    let mut s = Session::load(path, streamed); let out = s.render(frames); s.close(); out
}
fn ac_rms(frames: &[[f32; 2]], channel: usize) -> f64 {
    let mean = frames.iter().map(|f| f64::from(f[channel])).sum::<f64>() / frames.len() as f64;
    (frames.iter().map(|f| (f64::from(f[channel]) - mean).powi(2)).sum::<f64>() / frames.len() as f64).sqrt()
}
fn compare_late(label: &str, actual: &[[f32; 2]], reference: &[[f32; 2]]) {
    assert_eq!(actual.len(), reference.len()); assert!(actual.len() >= NOTE_FRAMES);
    let (actual, reference) = (&actual[actual.len() - 4096..], &reference[reference.len() - 4096..]);
    let expected_energy: f64 = reference.iter().flatten().map(|&v| f64::from(v).powi(2)).sum();
    assert!(expected_energy > 1e-5, "ordinary reference must contain a real late signal");
    let error: f64 = actual.iter().flatten().zip(reference.iter().flatten()).map(|(&a, &b)| (f64::from(a) - f64::from(b)).powi(2)).sum();
    let worst = actual.iter().flatten().zip(reference.iter().flatten()).fold(0f32, |p, (&a, &b)| p.max((a-b).abs()));
    let a = [ac_rms(actual, 0), ac_rms(actual, 1)];
    let r = [ac_rms(reference, 0), ac_rms(reference, 1)];
    eprintln!("{label}: late_frames={} actual_ac_rms={a:?} reference_ac_rms={r:?} normalized_pcm_error={} peak_error={worst}", actual.len(), error / expected_energy);
    assert!(error / expected_energy < 1e-8 && worst < 4e-6,
        "missing/misrouted late channel PCM versus independent ordinary stereo reference: {label}; normalized_error={}, peak_error={worst}", error / expected_energy);
    for channel in 0..2 {
        if r[channel] > 1e-4 { assert!((a[channel] / r[channel] - 1.).abs() < 1e-4); }
        else { assert!(a[channel] < 1e-6); }
    }
}
fn static_case(label: &str, source: Vec<[i16; 6]>, matrix: Matrix, streamed: bool) {
    let f = Fixture::new(); let reference = stereo_reference(&source, matrix);
    let wide_path = f.program("wide", &source, matrix, "");
    // The reference is an independent signed-integer stereo decode of the same
    // authored matrix. It is staged on the same six-channel working layout, so
    // both sides cross the ordinary wide outlet law identically and the ideal
    // integer arithmetic stays exact without modelling that conversion here.
    let staged: Vec<[i16; 6]> = reference.iter().map(|frame| [frame[0], frame[1], 0, 0, 0, 0]).collect();
    let ref_path = f.program("reference", &staged, identity(), "");
    let expected = render(&ref_path, streamed, NOTE_FRAMES);
    let actual = render(&wide_path, streamed, NOTE_FRAMES);
    compare_late(label, &actual, &expected);
}

#[test]
fn control_first_two_full() {
    static_case("first2/full", wide(true, false), [[2, -1, 0, 0, 0, 0], [1, 3, 0, 0, 0, 0]], false);
}
#[test]
fn control_first_two_streamed() {
    static_case("first2/streamed", wide(true, false), [[2, -1, 0, 0, 0, 0], [1, 3, 0, 0, 0, 0]], true);
}
#[test]
fn control_ordinary_mono_matches_duplicated_stereo() {
    let f = Fixture::new(); let mono: Vec<_> = (0..SOURCE_FRAMES).map(|i| [tone(i, 1, 0)]).collect();
    let stereo: Vec<_> = mono.iter().map(|f| [f[0]; 2]).collect();
    let mono = f.program("mono", &mono, identity(), "");
    let stereo = f.program("stereo", &stereo, identity(), "");
    for streamed in [false, true] { compare_late("mono/stereo control", &render(&mono, streamed, NOTE_FRAMES), &render(&stereo, streamed, NOTE_FRAMES)); }
}
macro_rules! orientation {
    ($name:ident, $input:expr, $output:expr, $streamed:expr) => {
        #[test] fn $name() {
            let mut matrix = [[0; 6]; 2]; matrix[$output - 1][$input - 1] = 4;
            static_case(stringify!($name), wide(false, true), matrix, $streamed);
        }
    };
}
orientation!(static_input3_output1_full, 3, 1, false);
orientation!(static_input3_output1_streamed, 3, 1, true);
orientation!(static_input3_output2_full, 3, 2, false);
orientation!(static_input3_output2_streamed, 3, 2, true);
orientation!(static_input4_output1_full, 4, 1, false);
orientation!(static_input4_output1_streamed, 4, 1, true);
orientation!(static_input4_output2_full, 4, 2, false);
orientation!(static_input4_output2_streamed, 4, 2, true);
orientation!(static_input5_output1_full, 5, 1, false);
orientation!(static_input5_output1_streamed, 5, 1, true);
orientation!(static_input5_output2_full, 5, 2, false);
orientation!(static_input5_output2_streamed, 5, 2, true);
orientation!(static_input6_output1_full, 6, 1, false);
orientation!(static_input6_output1_streamed, 6, 1, true);
orientation!(static_input6_output2_full, 6, 2, false);
orientation!(static_input6_output2_streamed, 6, 2, true);
#[test]
fn static_signed_body_full() {
    static_case("signed body/full", wide(false, true), [[0, 0, 2, -1, 1, 0], [0, 0, 0, 2, 0, -2]], false);
}
#[test]
fn static_signed_body_streamed() {
    static_case("signed body/streamed", wide(false, true), [[0, 0, 2, -1, 1, 0], [0, 0, 0, 2, 0, -2]], true);
}
fn cancellation(streamed: bool) {
    let mut source = wide(true, true);
    for frame in &mut source { frame[3] = frame[2]; }
    // Output1 cancels ch3/ch4 exactly while retaining a first-channel signal.
    // Output2 has an independent nonzero upper-channel witness: old first2-only
    // decoding cannot pass this test by producing silence in both outputs.
    static_case("signed cancellation + nonzero upper witness", source,
        [[1, 0, 2, -2, 0, 0], [0, 0, 0, 0, 2, -1]], streamed);
}
#[test] fn static_signed_cancellation_full() { cancellation(false); }
#[test] fn static_signed_cancellation_streamed() { cancellation(true); }

const LIVE_SCRIPT: &str = r#"
  function onController(e)
    if e.controller==20 then
      Program.layers[1].keygroups[1].inserts[1]:setParameter('Gain_3_1',1.0)
      controlChange(119,42)
    end
  end
"#;
#[test]
fn live_keygroup_gain_3_1_binding_exists_for_one_held_source() {
    let f = Fixture::new(); let path = f.program("binding", &wide(false, true), [[0; 6]; 2], LIVE_SCRIPT);
    let xml = std::fs::read_to_string(&path).unwrap();
    let doc = roxmltree::Document::parse(&xml).unwrap();
    let node = doc.descendants().find(|n| n.has_tag_name("GainMatrix")).unwrap().id().get_usize();
    let key = format!("uvi:insert:{node}:GainMatrix:Gain_3_1");
    let mut session = Session::load(&path, false); session.render(BEFORE_WRITE);
    // The legacy Pcm snapshot refuses channel-family sources by contract, so the
    // held source is observed through the physical voice count (already pinned
    // by Session::render) rather than a fabricated stereo projection.
    assert_eq!(session.rt.voice_count(), 1);
    let id = sampler_core::lower::ir_control_id(&key);
    let has_ir_control = session.ir.controls.iter().any(|c| c.key == key);
    let bindings = session.rt.engine_parameter_bindings(session.rt.active_plan()).unwrap();
    let has_runtime_binding = bindings.iter().any(|b| b.control == id);
    let current = session.rt.control_base_value(session.rt.active_plan(), id);
    eprintln!("held_keygroup_cell: physical_voices=1 ir_control={has_ir_control} runtime_binding={has_runtime_binding} current={current:?}");
    assert!(has_ir_control && has_runtime_binding && matches!(current, Ok(ControlValue::Real(v)) if v==0.),
        "keygroup Gain_3_1 lacks the normal live catalog-to-runtime control binding while its source is held");
    session.close();
}
fn held_write_case(path: &Path, reference: &Path, label: &str) {
    let expected = render(reference, false, BEFORE_WRITE + NOTE_FRAMES);
    let translated = sampler_uvi::translate_path(path).unwrap();
    let groups = translated.groups.clone();
    let loaded = sampler_uvi::assemble_translated(translated, RATE).unwrap();
    let xml = std::fs::read_to_string(path).unwrap();
    let host = ScriptHost::new(&xml, (), Default::default()).unwrap();
    assert!(host.fault_counts().init.is_empty(), "{:?}", host.findings());
    let mut driver = Driver::new(host, groups, RATE);
    let limits = Limits::for_plan(&loaded.plan, 4, 4);
    let mut rt = Runtime::new(loaded.plan, limits).unwrap();
    let note = rt.trigger(input(), 60, VELOCITY).unwrap();
    let mut before = vec![[0.; 2]; BEFORE_WRITE]; rt.render(&mut before).unwrap();
    assert!(before.iter().all(|v| *v==[0.; 2]));
    assert_eq!(rt.voice_count(), 1);
    let mut original = None;
    rt.visit_onset_voices(|v| { original = Some((v.voice.runtime, v.voice.slot, v.voice.generation)); true });
    driver.input(&rt, HostInput::Controller { cc: 20, value: 127, channel: 0 });
    driver.wake(&mut rt).unwrap();
    let mut fences = 0; driver.drain_midi(|m| { if m.status==0xb0 && m.a==119 && m.b==42 { fences+=1; } });
    assert_eq!(fences, 1, "ordinary controller callback must execute exactly once");
    let mut after = vec![[0.; 2]; NOTE_FRAMES];
    for block in after.chunks_mut(64) { driver.wake(&mut rt).unwrap(); rt.render(block).unwrap(); }
    assert_eq!(rt.voice_count(), 1);
    let mut final_voice = None;
    rt.visit_onset_voices(|v| { final_voice = Some((v.voice.runtime, v.voice.slot, v.voice.generation)); true });
    assert_eq!(original, final_voice, "write must affect the held source, without retrigger/replacement");
    assert_eq!(rt.stats().nonfinite_frames, 0); assert_eq!(rt.stats().stream_underruns, 0);
    assert_eq!(rt.stats().voice_drops, 0); assert!(rt.take_stream_fault().is_none());
    rt.release(note).unwrap(); rt.render(&mut [[0.; 2]; 512]).unwrap(); rt.flush_ended(|_| true);
    assert_eq!(rt.voice_count(), 0);
    compare_late(label, &after, &expected[BEFORE_WRITE..]);
    assert!(driver.unmodeled().is_empty(), "{:?}", driver.unmodeled());
}

#[test]
fn control_live_program_matrix_input1_opens_same_held_stereo_note() {
    let f = Fixture::new();
    let stereo: Vec<_> = (0..SOURCE_FRAMES).map(|i| [tone(i, 1, 0), 0]).collect();
    let script = LIVE_SCRIPT.replace("Program.layers[1].keygroups[1].inserts[1]", "Program.inserts[1]")
        .replace("Gain_3_1", "Gain_1_1");
    let path = f.program("live_stereo_control", &stereo, identity(), &script);
    let xml = std::fs::read_to_string(&path).unwrap().replace("<Program Gain=\"0.75\">",
        &format!("<Program Gain=\"0.75\"><Inserts>{}</Inserts>", matrix_xml([[0; 6]; 2])));
    std::fs::write(&path, xml).unwrap();
    let reference = f.program("live_stereo_reference", &stereo, identity(), "");
    held_write_case(&path, &reference, "positive ordinary stereo program-matrix live write");
}
#[test]
fn live_keygroup_gain_3_1_opens_the_same_held_note_via_public_driver() {
    let f = Fixture::new(); let source = wide(false, true);
    let path = f.program("live", &source, [[0; 6]; 2], LIVE_SCRIPT);
    let mut target = [[0; 6]; 2]; target[0][2] = 4;
    // Match the static cases: stage the independent stereo oracle on the same
    // six-channel working layout so both sides cross the ordinary wide outlet
    // law identically instead of modelling its 1/sqrt(3) reduction here.
    let staged: Vec<[i16; 6]> = stereo_reference(&source, target).iter()
        .map(|frame| [frame[0], frame[1], 0, 0, 0, 0]).collect();
    let reference = f.program("live_reference", &staged, identity(), "");
    held_write_case(&path, &reference, "live keygroup Gain_3_1 after ordinary controller");
}
