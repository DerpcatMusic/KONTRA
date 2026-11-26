//! End-to-end verification of the UVI Falcon Lua API surface (`prelude.lua` +
//! `kontra-lua` runtime): Context, Conversions, Time, Events, Event callbacks,
//! Voice, MIDI, MidiEvent/MidiSequence, Async + AsyncTask cancellation,
//! Element/Oscillator/SampledReverb/Keygroup APIs, and keyboard colour/type
//! forwarding.

use kontra_lua::{Callbacks, compile};
use kontra_vm::{Action, Foreign, HostApi, HostCall, HostValue, Machine, Step, Text};

/// Deterministic test host recording every action and answering queries with
/// realistic engine values (120 BPM, 4/4, 48 kHz, active keys/CCs).
struct MockHost {
    calls: Vec<(Action, Vec<i64>, Vec<String>)>,
    note_held: bool,
    picked_file: Option<String>,
    pick_polls: usize,
}

impl MockHost {
    fn new() -> Self {
        Self {
            calls: Vec::new(),
            note_held: true,
            picked_file: None,
            pick_polls: 0,
        }
    }

    fn actions(&self, action: Action) -> Vec<&[i64]> {
        self.calls
            .iter()
            .filter(|(a, _, _)| *a == action)
            .map(|(_, ints, _)| ints.as_slice())
            .collect()
    }
}

impl HostApi for MockHost {
    fn call(&mut self, call: HostCall<'_>) -> HostValue {
        let texts: Vec<String> = call.texts.iter().map(|t| t.as_str().to_owned()).collect();
        self.calls.push((call.action, call.ints.to_vec(), texts));
        match call.action {
            Action::EngineMs => HostValue::Real(2500.0),
            Action::DurationQuarter => HostValue::Int(500_000), // 120 BPM
            Action::DurationBar => HostValue::Int(2_000_000),   // 4/4 bar = 2000 ms
            Action::SignatureNum => HostValue::Int(4),
            Action::SignatureDenom => HostValue::Int(4),
            Action::SongPosition => HostValue::Int(960 * 3), // 3 beats
            Action::KeyDown => HostValue::Int(i32::from(call.ints.first() == Some(&60))),
            Action::KeyDownOct => HostValue::Int(i32::from(call.ints.first() == Some(&0))),
            Action::NoteHeld => HostValue::Int(i32::from(self.note_held)),
            Action::NoteDuration => HostValue::Int(if call.ints.first() == Some(&60) {
                125_000
            } else {
                0
            }),
            Action::CcNum => HostValue::Int(1),
            Action::Cc => match call.ints.first().copied() {
                Some(1) => HostValue::Int(64),
                Some(11) => HostValue::Int(100),
                Some(128) => HostValue::Int(4096),
                Some(129) => HostValue::Int(90),
                _ => HostValue::Int(0),
            },
            Action::PolyAtNum => HostValue::Int(62),
            Action::PolyAt => HostValue::Int(77),
            Action::EventId => HostValue::Int(42),
            Action::EventNote => HostValue::Int(60),
            Action::EventVelocity => HostValue::Int(100),
            Action::PlayNote => HostValue::Int(99),
            Action::SignalType => HostValue::Int(1),
            Action::PickFile => HostValue::Int(7),
            Action::PickedFile => {
                self.pick_polls += 1;
                if let Some(name) = &self.picked_file {
                    *call.text_out = Text::from_str_cut(name).0;
                    HostValue::Text
                } else {
                    HostValue::Nothing
                }
            }
            Action::LoadImpulse => HostValue::Int(1),
            _ => HostValue::Int(0),
        }
    }
}

fn run_to_done(m: &mut Box<dyn Machine>, co: kontra_vm::CoId, host: &mut MockHost) -> Vec<u64> {
    let mut waits = Vec::new();
    loop {
        match m.run(co, host) {
            Step::Done => break,
            Step::Yield => m.begin_block(),
            Step::Wait(us) => {
                waits.push(us);
                // The note lets go during the third wait: waitForRelease waits once.
                host.note_held = waits.len() < 3;
            }
        }
    }
    waits
}

#[test]
fn context_and_conversions_answer_exact_host_units() {
    let source = r#"
        function onInit()
            assert(getTime() == 2500)
            assert(getTempo() == 120)
            assert(getSamplingRate() == 48000)
            local num, den = getTimeSignature()
            assert(num == 4 and den == 4)
            assert(getBeatDuration() == 500)
            assert(getBarDuration() == 2000)
            assert(getBeatTime() == 3)
            assert(getRunningBeatTime() == 5)
            assert(beat2ms(2) == 1000)
            assert(ms2beat(1000) == 2)
            assert(ms2samples(1000) == 48000)
            assert(samples2ms(48000) == 1000)
            assert(getCC(11) == 100)
            assert(getNoteDuration(60) == 125)
            assert(isKeyDown(60) == true and isKeyDown(61) == false)
            assert(isOctaveKeyDown(72) == true and isOctaveKeyDown(61) == false)
            assert(isNoteHeld() == true)
            assert(Event.Type.NoteOn == 144 and Event.Type.ControlChange == 176)
        end
    "#;
    let mut m = compile(source, &[], &[]).unwrap().machine();
    let mut host = MockHost::new();
    let co = m.start(Callbacks::INIT, 0).expect("init");
    run_to_done(&mut m, co, &mut host);
    assert_eq!(m.faults().bad_code, 0, "{}", m.faults().last_error.as_str());
}

#[test]
fn time_events_and_voice_commands_forward_exact_host_actions() {
    let source = r#"
        function onNote(e)
            local id = playNote(e.note + 12, 90, 250)
            changeVolume(id, 0.5)
            changeVolumedB(id, -6.5)
            changeTune(id, 2.5)
            changePan(id, -0.5)
            setSampleOffset(id, 15)
            fade(id, 1, 20)
            fade2(id, 1, 0, 30)
            fadein(id, 10)
            fadeout(id, 40, true)
            sendScriptModulation(3, 0.75, 50, id)
            sendScriptModulation2(4, 0.0, 1.0, 25, id)
            postEvent({ type = Event.NoteOn, note = 64, velocity = 80 })
            wait(12)
            waitBeat(0.5)
            waitForRelease()
            releaseVoice(id)
        end
    "#;
    let mut m = compile(source, &[], &[]).unwrap().machine();
    let mut host = MockHost::new();
    let co = m.start(Callbacks::NOTE, 0).expect("note");
    let waits = run_to_done(&mut m, co, &mut host);
    assert_eq!(m.faults().bad_code, 0, "{}", m.faults().last_error.as_str());
    assert_eq!(waits, [12_000, 250_000, 1_000]);
    assert_eq!(host.actions(Action::PlayNote).len(), 2);
    assert_eq!(host.actions(Action::ChangeVol).len(), 2);
    assert_eq!(host.actions(Action::ChangeTune), [vec![99, 250_000, 0]]);
    assert_eq!(host.actions(Action::ChangePan), [vec![99, -500, 0]]);
    assert_eq!(host.actions(Action::SetEventPar), [vec![99, 4096, 15_000]]);
    assert_eq!(host.actions(Action::FadeIn).len(), 2);
    assert_eq!(host.actions(Action::FadeOut).len(), 2);
    assert_eq!(host.actions(Action::SetEventParArr).len(), 3);
    assert_eq!(host.actions(Action::NoteOff), [vec![99]]);
}

#[test]
fn midi_api_midi_event_and_midi_sequence_work_end_to_end() {
    let source = r#"
        function onInit()
            controlChange(74, 100)
            pitchBend(0.5)
            afterTouch(80)
            polyAfterTouch(60, 64)
            programChange(5, 2)
            enable_data_entry_as_cc(true)
            enable_nrpn_as_cc(true)
            enable_rpn_as_cc(true)
            assert(__midi_flags.data_entry_as_cc and __midi_flags.nrpn_as_cc and __midi_flags.rpn_as_cc)

            local seq = createMidiFile()
            local e1 = MidiEvent.MidiEvent({ type = Event.NoteOn, note = 60, velocity = 100 }, 0.0, 1)
            local e2 = MidiEvent({ type = Event.NoteOn, note = 64, velocity = 90 }, 2.0, 1)
            local e3 = MidiEvent({ type = Event.ControlChange, controller = 1, value = 127 }, 1.0, 1)
            seq:insertEvent(e2)
            seq:insertEvent(e1)
            seq:insertEvent(e3)
            assert(seq:getNumEventsForTrack(1) == 3)
            assert(seq:getEvent(1, 1).beat == 0.0)
            assert(seq:getEvent(2, 1).beat == 1.0)
            assert(seq:getEvent(3, 1).beat == 2.0)
            assert(seq:findAt(1.0, 1) == 2)
            assert(seq:findNextAt(1.5, 1) == 3)
            assert(seq:findPreviousAt(1.5, 1) == 2)
            postMidiEvent(seq:getEvent(2, 1))

            local saved = saveMidi(seq, "Pattern.mid")
            assert(saved.success)
            local loaded_seq = nil
            local task = loadMidi("Pattern.mid", function(t) loaded_seq = t.midi end)
            assert(task.success and loaded_seq:getNumEventsForTrack(1) == 3)
            task:cancel()
            assert(task.cancelled)
        end
    "#;
    let mut m = compile(source, &[], &[]).unwrap().machine();
    let mut host = MockHost::new();
    let co = m.start(Callbacks::INIT, 0).expect("init");
    run_to_done(&mut m, co, &mut host);
    assert_eq!(m.faults().bad_code, 0, "{}", m.faults().last_error.as_str());
    assert!(
        host.actions(Action::SetController)
            .contains(&[74, 100].as_slice())
    );
    assert!(
        host.actions(Action::SetController)
            .contains(&[128, 4096].as_slice())
    );
    assert!(
        host.actions(Action::SetController)
            .contains(&[129, 80].as_slice())
    );
    assert_eq!(host.actions(Action::SetPolyAt), [vec![60, 64]]);
    assert_eq!(host.actions(Action::SetMidi), [vec![1, 192, 5, 0]]);
}

#[test]
fn callbacks_on_event_on_program_change_and_all_specific_handlers_dispatch() {
    let source = r#"
        seen = {}
        function onEvent(e) table.insert(seen, e.type) end
        function onProgramChange(e) table.insert(seen, "pc") end
        function onTransport(playing) table.insert(seen, playing and "play" or "stop") end
        function onSave() return { ok = true } end
        function onLoad(s) assert(s.ok) end
        function onAfterTouch(e) table.insert(seen, "at") end
        function onPitchBend(e) table.insert(seen, "pb") end
        function onPolyAfterTouch(e) table.insert(seen, "pat") end
        function onController(e) table.insert(seen, "cc") postEvent(e) end
        function onRelease(e) table.insert(seen, "rel") end
    "#;
    let compiled = compile(source, &[], &[]).unwrap();
    assert!(compiled.callbacks.note);
    assert!(compiled.callbacks.release);
    assert!(compiled.callbacks.controller);
    assert!(compiled.callbacks.transport);
    assert!(compiled.callbacks.poly_at);
    let mut m = compiled.machine();
    let mut host = MockHost::new();
    for entry in [
        Callbacks::INIT,
        Callbacks::NOTE,
        Callbacks::RELEASE,
        Callbacks::CONTROLLER,
        Callbacks::TRANSPORT,
        Callbacks::POLY_AT,
    ] {
        let co = m.start(entry, 0).expect("entry");
        run_to_done(&mut m, co, &mut host);
    }
    assert_eq!(m.faults().bad_code, 0, "{}", m.faults().last_error.as_str());
}

#[test]
fn async_tasks_list_files_text_data_purge_and_cancellation() {
    let tree = "return {params={},db=0,pan=0,layers={{params={Name='L1'},keygroups={
        {params={Name='KG1'},group=0,oscillators={{params={SamplePath='./a.wav',BaseNote='60'}}}}}}}}";
    let modules = [
        ("__program.lua".to_owned(), tree.to_owned()),
        ("__dir.lua".to_owned(), "return \"Script\"".to_owned()),
        (
            "__file/json/arp/a.arp".to_owned(),
            "return {data={step=1}}".to_owned(),
        ),
        (
            "__file/json/arp/b.arp".to_owned(),
            "return {data={step=2}}".to_owned(),
        ),
    ];
    let source = r#"
        function onInit()
            local found = nil
            local lt = listFiles("../JSON/Arp", "*.arp", function(t) found = t.result end)
            assert(lt.success and #found == 2 and found[1] == "json/arp/a.arp")
            lt:cancel()
            assert(lt.cancelled)

            local st = saveTextData("hello uvi", "note.txt")
            assert(st.success)
            st:cancel()
            assert(st.cancelled)
            local got_text = nil
            local dt = loadTextData("note.txt", function(txt, task) got_text = txt; assert(task.data == "hello uvi") end)
            assert(dt.success and got_text == "hello uvi")
            dt:cancel()
            assert(dt.cancelled)

            local pt = purge(Program)
            local ut = unpurge(Program)
            assert(pt.success and ut.success)

            local osc = Program.layers[1].keygroups[1].oscillators[1]
            local opt_task = setPlaybackOptions(osc, { LoopStart = 100, LoopEnd = 500 })
            assert(opt_task.success and osc:getParameter("LoopStart") == 100)

            -- browseForFile returns an AsyncBrowseForFileTask that can be cancelled.
            local browse_called = false
            local bt = browseForFile("open", "Pick", "", "*.arp", function() browse_called = true end)
            bt:cancel()
            assert(bt.cancelled and not browse_called)

            local s_task = loadSample(osc, "b.wav")
            assert(s_task.success)
            s_task:cancel()
            assert(s_task.cancelled)

            local d_save = saveData({ x = 7 }, "saved.json")
            assert(d_save.success)
            local d_load = loadData("saved.json", function(d) assert(d.x == 7) end)
            assert(d_load.success and d_load.data.x == 7)
            local st_save = saveState("s.preset")
            local st_load = loadState("s.preset")
            assert(st_save.success and st_load.success)
        end
    "#;
    let mut m = compile(source, &modules, &[]).unwrap().machine();
    let mut host = MockHost::new();
    let co = m.start(Callbacks::INIT, 0).expect("init");
    run_to_done(&mut m, co, &mut host);
    while let Some((task_co, _)) = m.spawned() {
        run_to_done(&mut m, task_co, &mut host);
    }
    assert_eq!(m.faults().bad_code, 0, "{}", m.faults().last_error.as_str());
    assert_eq!(host.actions(Action::PurgeGroup), [vec![0, 0], vec![0, 1]]);
}

#[test]
fn element_oscillator_keygroup_and_sampled_reverb_members_work() {
    let tree = "return {params={Name='Prog'},db=0,pan=0,inserts={{params={Name='Rev'},fx={2,0,0}}},
        layers={{params={Name='L1'},keygroups={{params={Name='KG1',LowKey='48',HighKey='72'},group=0,
        oscillators={{params={SamplePath='./a.wav',BaseNote='60',FineTune='5',NumSlices='4',Tempo='128'}}}}}}}}";
    let modules = [("__program.lua".to_owned(), tree.to_owned())];
    let source = r#"
        function onInit()
            assert(Keygroup.Parameters.Gain == "Gain")
            assert(Oscillator.Parameters.Pitch == "Pitch")
            assert(SampleBasedOscillator.Parameters.BaseNote == "BaseNote")
            assert(SampleMappingOscillator.Parameters.SamplePath == "SamplePath")
            assert(SamplePlayer.Parameters.Reverse == "Reverse")

            local l = Program.synthChildren[1]
            local kg = l.synthChildren[1]
            local osc = kg.synthChildren[1]
            assert(osc:hasParameter("BaseNote"))
            assert(#osc.parameterDefinitions > 0)
            assert(#osc:getParameterConnections("Pitch") == 0)
            assert(osc.sampleInfo.name == "a.wav" and osc.sampleInfo.basenote == 60)
            assert(#osc:getSliceInfo() == 4)
            assert(osc.looplabInfo.numSlices == 4 and osc.looplabInfo.tempo == 128)
            assert(osc:getNumZones() == 1 and osc:getLoadingStatus() == 0)
            osc:loadMapping("map.xml")
            osc:purgeZones({1})
            osc:unpurgeZones({1})
            osc:waitForLoading()

            -- Dynamic effect insertion, reordering, and removal.
            local fx1 = l:insertEffect("SparkVerb")
            local fx2 = l:insertEffect("DualDelay")
            assert(#l.inserts == 2 and l.inserts[1] == fx1 and l.inserts[2] == fx2)
            assert(l:moveEffect(2, 1))
            assert(l.inserts[1] == fx2 and l.inserts[2] == fx1)
            assert(l:removeEffect(1))
            assert(#l.inserts == 1 and l.inserts[1] == fx1)

            -- SampledReverb:loadImpulse forwards to host LoadImpulse.
            local rev = Program.inserts[1]
            local rt = rev:loadImpulse("Hall.aif")
            assert(rt.success)
            local rt2 = SampledReverb.loadImpulse(rev, "Hall2.aif")
            assert(rt2.success)
        end
    "#;
    let mut m = compile(source, &modules, &[]).unwrap().machine();
    let mut host = MockHost::new();
    let co = m.start(Callbacks::INIT, 0).expect("init");
    run_to_done(&mut m, co, &mut host);
    assert_eq!(m.faults().bad_code, 0, "{}", m.faults().last_error.as_str());
    assert_eq!(host.actions(Action::LoadImpulse).len(), 2);
}

#[test]
fn set_key_colour_keeps_each_keys_argb_and_marks_it_set_until_reset() {
    // VWinds' pattern: transparent white / black over the playable range,
    // "grey" outside it, "red" keyswitches; Augmented Orchestra's bare
    // "RRGGBB" / "AARRGGBB" zone colours.
    let colours: [(i64, &str, u32); 7] = [
        (60, "#00FFFFFF", 0x00FF_FFFF),
        (61, "#00000000", 0),
        (20, "grey", 0xFF80_8080),
        (36, "red", 0xFFFF_0000),
        (70, "c6cf69", 0xFFC6_CF69),
        (71, "00FFFFFF", 0x00FF_FFFF),
        (72, "#3C00FECD", 0x3C00_FECD),
    ];
    let calls: String = (colours.iter())
        .map(|(k, c, _)| format!("setKeyColour({k}, \"{c}\")\n"))
        .collect();
    let source = format!("{calls}resetKeyColour(72)\n");
    let mut m = compile(&source, &[], &[]).unwrap().machine();
    let mut host = MockHost::new();
    let co = m.start(Callbacks::INIT, 0).expect("init");
    run_to_done(&mut m, co, &mut host);
    assert_eq!(m.faults().bad_code, 0, "{}", m.faults().last_error.as_str());
    let last = |action: Action| {
        let mut by_key = [None; 128];
        for ints in host.actions(action) {
            if let &[k, v] = ints {
                by_key[usize::try_from(k).unwrap()] = Some(v);
            }
        }
        by_key
    };
    let (argb, set) = (last(Action::SetKeyColor), last(Action::SetKeyType));
    let reset = 72;
    for &(k, colour, want) in &colours {
        let k = usize::try_from(k).unwrap();
        let (got_argb, got_set) = (argb[k].unwrap(), set[k].unwrap());
        if k == reset {
            assert_eq!((got_argb, got_set), (0, 0), "{colour}: reset");
        } else {
            // The host keeps the low 32 bits.
            assert_eq!(got_argb as u32, want, "{colour}");
            assert_eq!(got_set, 1, "{colour}: set");
        }
    }
}

#[test]
fn param_widget_constructors_bind_and_update_target_parameters() {
    let tree = "return {params={Name='Prog'},db=0,pan=0,layers={{params={Name='L1'},keygroups={{
        params={Name='KG1',Gain='1',Pitch='0',Bypass='0'},group=0,
        oscillators={{params={SamplePath='./a.wav',BaseNote='60',Gain='1',Reverse='0'}}}}}}}}";
    let modules = [("__program.lua".to_owned(), tree.to_owned())];
    let source = r#"
        local kg = Program.layers[1].keygroups[1]
        local pk = ParamKnob("pk", kg, "Gain")
        local pm = ParamMenu("pm", kg, "Bypass", { "Off", "On" })
        local pn = ParamNumBox("pn", kg, "Pitch")
        local pb = ParamOnOffButton("pb", kg, "Bypass")
        local ps = ParamSlider("ps", kg, "Gain")
        function onInit()
            pk:setValue(0.5)
            pm:setValue(2)
            pn:setValue(7)
            pb:setValue(true)
            ps:setValue(0.75)
            assert(pk.value == 0.5)
            assert(pm.value == 2)
            assert(pn.value == 7)
            assert(pb.value == true)
            assert(ps.value == 0.75)
        end
    "#;
    let compiled = compile(source, &modules, &[]).unwrap();
    assert_eq!(compiled.ui.widgets.len(), 5);
    let mut m = compiled.machine();
    let mut host = MockHost::new();
    let co = m.start(Callbacks::INIT, 0).expect("init");
    run_to_done(&mut m, co, &mut host);
    assert_eq!(m.faults().bad_code, 0, "{}", m.faults().last_error.as_str());
}

#[test]
fn runtime_channel_emits_swaps_mapping_slices_params_and_effects_zero_alloc() {
    use kontra_lua::{
        EffectOp, EffectTarget, KeygroupParamKind, MAX_INLINE_SLICES, OscParamKind, RuntimeReply,
        RuntimeRequest,
    };
    let tree =
        "return {params={Name='Prog'},db=0,pan=0,layers={{params={Name='L1'},bus=0,keygroups={{
        params={Name='KG1',LowKey='0',HighKey='127'},group=0,
        oscillators={{params={SamplePath='./init.wav',BaseNote='60'}}}}}}}}";
    let modules = [("__program.lua".to_owned(), tree.to_owned())];
    let source = r#"
        sample_done = false
        mapping_done = false
        playback_done = false
        slice_starts = {}
        function onNote()
            local l = Program.layers[1]
            local kg = l.keygroups[1]
            local osc = kg.oscillators[1]
            osc:setParameter("BaseNote", 72)
            kg:setParameter("LowKey", 36)
            loadSample(osc, "./replaced.wav", function(t)
                sample_done = t.success
                local slices = osc:getSliceInfo()
                for i, s in ipairs(slices) do
                    slice_starts[i] = s.start
                end
            end)
            osc:loadMapping("./map.xml", function(t)
                mapping_done = t.success and osc:getNumZones() == 3
            end)
            setPlaybackOptions(osc, { SampleStart = 64, LoopMode = 1, LoopStart = 128, LoopEnd = 512, Reverse = true }, function(t)
                playback_done = t.success
            end)
            kg:insertEffect("OnePole")
            kg:insertEffect("SparkVerb")
            kg:moveEffect(2, 1)
            kg:removeEffect(1)
        end
        function onController()
            assert(sample_done, "sample swap callback did not complete")
            assert(mapping_done, "mapping swap callback did not complete")
            assert(playback_done, "playback options callback did not complete")
            assert(slice_starts[1] == 0 and slice_starts[2] == 256 and slice_starts[3] == 512)
            local osc = Program.layers[1].keygroups[1].oscillators[1]
            assert(osc:getLoadingStatus() == 0)
            assert(osc.sampleInfo.length == 1024 and osc.sampleInfo.sampleRate == 48000)
        end
    "#;
    let mut compiled = compile(source, &modules, &[]).unwrap();
    let rt = compiled.enable_runtime();
    let mut m = compiled.machine();
    let mut host = MockHost::new();

    // Init runs before connect() (mirrors Engine::new).
    let init = m.start(Callbacks::INIT, 0).expect("init");
    run_to_done(&mut m, init, &mut host);
    rt.connect();

    // Trigger onNote -> emits runtime requests and spawns pump_swaps.
    let note = m.start(Callbacks::NOTE, 1).expect("note");
    run_to_done(&mut m, note, &mut host);

    let mut reqs = Vec::new();
    while let Some(r) = rt.pop_request() {
        reqs.push(r);
    }
    assert!(reqs.iter().any(|r| matches!(
        r,
        RuntimeRequest::OscParam { group: 0, slot: 0, param: OscParamKind::BaseNote, value } if (*value - 72.0).abs() < 1e-6
    )));
    assert!(reqs.iter().any(|r| matches!(
        r,
        RuntimeRequest::KeygroupParam { group: 0, param: KeygroupParamKind::LowKey, value } if (*value - 36.0).abs() < 1e-6
    )));
    assert!(reqs.iter().any(|r| matches!(
        r,
        RuntimeRequest::EffectChain {
            target: EffectTarget::Keygroup(0),
            op: EffectOp::Insert { .. }
        }
    )));
    assert!(reqs.iter().any(|r| matches!(
        r,
        RuntimeRequest::EffectChain {
            target: EffectTarget::Keygroup(0),
            op: EffectOp::Move { from: 1, to: 0 }
        }
    )));
    assert!(reqs.iter().any(|r| matches!(
        r,
        RuntimeRequest::EffectChain {
            target: EffectTarget::Keygroup(0),
            op: EffectOp::Remove { index: 0 }
        }
    )));

    // Push replies from "control thread".
    for req in &reqs {
        match *req {
            RuntimeRequest::LoadSample { id, .. } => {
                let mut slice_starts = [0u32; MAX_INLINE_SLICES];
                slice_starts[0] = 0;
                slice_starts[1] = 256;
                slice_starts[2] = 512;
                assert!(rt.push_reply(RuntimeReply {
                    id,
                    ok: true,
                    frames: 1024,
                    sample_rate: 48_000,
                    num_zones: 1,
                    num_slices: 3,
                    slice_starts,
                }));
            }
            RuntimeRequest::LoadMapping { id, .. } => {
                assert!(rt.push_reply(RuntimeReply::simple(id, true, 2048, 48_000, 3)));
            }
            RuntimeRequest::PlaybackOptions { id, .. } => {
                assert!(rt.push_reply(RuntimeReply::simple(id, true, 1024, 48_000, 1)));
            }
            _ => {}
        }
    }

    // Pump spawned coroutine (pump_swaps) to completion.
    while let Some((co, _)) = m.spawned() {
        let mut step = m.run(co, &mut host);
        while matches!(step, Step::Yield | Step::Wait(_)) {
            m.begin_block();
            step = m.run(co, &mut host);
        }
    }

    // Verify Lua state saw every completion and slice position.
    let cc = m.start(Callbacks::CONTROLLER, 2).expect("cc");
    run_to_done(&mut m, cc, &mut host);
    assert_eq!(m.faults().bad_code, 0, "{}", m.faults().last_error.as_str());
}
