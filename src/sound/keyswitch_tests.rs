use super::*;
use crate::sound::event::HostPattern;
use crate::sound::articulation::{Input as Trigger, Overlay, Routing, identities};

fn fixture(owner: ir::SwitchOwner) -> (V2Core, ir::Instrument) {
    let mut inst = ir::Instrument {
        assets: (0..3).map(|n| ir::Asset { location: ir::AssetLocation::Path(format!("{n}.wav")), encoding: ir::Encoding::Wav, root_key: None, loops: Vec::new() }).collect(),
        groups: (0..3).map(|n| ir::Group { name: format!("g{n}"), ..Default::default() }).collect(),
        articulations: (0..3).map(|n| ir::Articulation { source: format!("source-axis:{n}"), name: format!("Art {n}"), switch_keys: vec![24 + n], default: n == 1, ..Default::default() }).collect(),
        switching: ir::Switching { owner, ..Default::default() },
        ..Default::default()
    };
    inst.zones = (0..3).map(|n| ir::Zone { group: Some(ir::GroupRef(n)), keys: ir::KeyRange { low: 48, high: 60 }, articulation: (owner == ir::SwitchOwner::Native).then_some(ir::ArticulationRef(n)), velocity: ir::VelocityResponse::None, ..ir::Zone::new(ir::AssetRef(n)) }).collect();
    if owner == ir::SwitchOwner::Behavior {
        inst.behaviors.push(ir::Behavior { name: "switches".into(), language: ir::Language::Ksp, slot: Some(0), state: Vec::new(), requires: Vec::new(), source: "on init\n declare $choice := 1\nend on\non note\n if (in_range($EVENT_NOTE,24,26))\n $choice := $EVENT_NOTE - 24\n ignore_event($EVENT_ID)\n exit\n end if\n disallow_group($ALL_GROUPS)\n allow_group($choice)\nend on".into() });
    }
    inst.assign_alternatives(32);
    let pcm = (0..3).map(|n| Pcm::new(48000, vec![[0.125 * (n + 1) as f32; 2]; 4096].into_boxed_slice()).unwrap()).collect();
    let loaded = sampler_kontakt::prepare(inst.clone(), pcm, &Default::default()).unwrap();
    let limits = limits(&loaded.plan).0;
    let mut part = Part::new(Runtime::new(loaded.plan, limits).unwrap(), MixTree::instrument("fixture")).unwrap();
    part.set_drivers(&inst);
    part.articulations = Some(1);
    if owner == ir::SwitchOwner::Behavior { part.tap_keys = Some(vec![Some(24), Some(25), Some(26)]); }
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, Some(Box::new(part)));
    (core, inst)
}

fn route(core: &mut V2Core, inst: &ir::Instrument, overlay: &Overlay) {
    let (keys, switching) = overlay.routing(inst, 0).unwrap();
    let mut mix = Mix::default();
    mix.articulation_routes.push(Some(Arc::new(Routing { keys, switching })));
    core.set_mix(&mix);
}

fn tap(core: &mut V2Core, key: u8) {
    core.event(0, Event::midi1(0x90, key, 100));
    core.render(16);
    core.event(0, Event::midi1(0x80, key, 0));
    core.render(16);
}

#[test]
fn keyswitch_remapped_input_selects_native_and_authored_articulations() {
    for owner in [ir::SwitchOwner::Native, ir::SwitchOwner::Behavior] {
        let (mut core, inst) = fixture(owner);
        let source = inst.clone();
        let ids = identities(&inst.articulations);
        let mut overlay = Overlay::default();
        overlay.set(&ids[0], Trigger::Keys(vec![49]));
        route(&mut core, &inst, &overlay);
        tap(&mut core, 49);
        assert_eq!(core.articulation(0), Some(0), "remapped key selects the existing articulation ({owner:?})");
        assert!(core.select_articulation(0, 2));
        core.render(16);
        tap(&mut core, 24);
        assert_eq!(core.articulation(0), Some(2), "replaced original is swallowed by default");
        overlay.keep_originals = true;
        route(&mut core, &inst, &overlay);
        tap(&mut core, 24);
        assert_eq!(core.articulation(0), Some(0), "explicit keep policy retains the original");
        core.event(0, Event::midi1(0x90, 60, 100));
        let rendered = core.render(64);
        assert!(rendered.live[0] && rendered.buses[0][0][..64].iter().any(|v| v.abs() > 1e-5), "selected articulation actually sounds");
        assert_eq!(inst, source, "input editing never mutates source IR");
    }
}

#[test]
fn keyswitch_routes_survive_rack_growth_reload_and_later_slot_updates() {
    for owner in [ir::SwitchOwner::Native, ir::SwitchOwner::Behavior] {
        let (mut core, inst) = fixture(owner);
        let mut overlay = Overlay::default();
        overlay.set(&identities(&inst.articulations)[0], Trigger::Keys(vec![49]));
        route(&mut core, &inst, &overlay);
        let mut grown = V2Core::with_parts(33, 48000.);
        core.adopt(&mut grown);
        let (mut replacement, _) = fixture(owner);
        let _retired = core.install(0, replacement.parts[0].take());
        tap(&mut core, 49);
        assert_eq!(core.articulation(0), Some(0), "growth keeps the remap through reload ({owner:?})");
        let (keys, switching) = overlay.routing(&inst, 0).unwrap();
        let mut mix = Mix::default();
        mix.parts.resize(33, Default::default());
        mix.articulation_routes.resize(33, None);
        mix.articulation_routes[32] = Some(Arc::new(Routing { keys, switching }));
        core.set_mix(&mix);
        let (mut replacement, _) = fixture(owner);
        let _retired = core.install(32, replacement.parts[0].take());
        core.play(32, Event::midi1(0x90, 49, 100));
        core.render(16);
        assert_eq!(core.articulation(32), Some(0), "new slots retain their own route after install ({owner:?})");
    }
}

#[test]
fn keyswitch_custom_alternatives_and_held_notes_survive_updates() {
    let (mut core, mut inst) = fixture(ir::SwitchOwner::Native);
    let ids = identities(&inst.articulations);
    inst.articulations[0].alternatives.channel = Some(7);
    let mut overlay = Overlay { driver: Some(ir::Driver::Channel as u8), ..Default::default() };
    route(&mut core, &inst, &overlay);
    core.event(0, Event::midi1(0x97, 60, 100));
    core.render(16);
    assert_eq!(core.articulation(0), Some(0), "raw incoming channel selects before manager-channel normalization");
    let note = HostNote { port: 0, channel: 0, key: 49, id: 42, clap: true };
    overlay.driver = Some(ir::Driver::Keys as u8);
    route(&mut core, &inst, &overlay);
    core.event(0, Event::NoteOn { note, velocity: 1., tune: 0. });
    assert!(core.key_held(0, 49));
    overlay.set(&ids[0], Trigger::Keys(vec![49]));
    route(&mut core, &inst, &overlay);
    core.event(0, Event::NoteOff(HostPattern { port: 0, channel: 0, key: 49, id: 42, clap: true }));
    core.render(256);
    core.end_block(256, &mut |_| true);
    assert!(!core.key_held(0, 49), "held playable note releases by its original ownership");
    let (keys, switching) = overlay.routing(&inst, 0).unwrap();
    assert!(switching.key_input(49).is_some());
    assert!(keys.iter().any(|k| k.key == 24), "source metadata retained in plan; input suppression occurs before admission");
    let mut invalid = overlay;
    invalid.set(&ids[0], Trigger::Keys(vec![128]));
    assert!(invalid.routing(&inst, 0).is_err(), "malformed persisted inputs are rejected");
}

/// Installed-library probe; skips only when the library is absent.
#[test]
fn keyswitch_real_afflatus_remap_routes_into_the_authored_script() {
    let relative = "Afflatus Chapter II Brass/Instruments/1. Ensembles/Multi Instruments/2 Horns KS.nki";
    let roots = std::env::var_os("KONTRA_KONTAKT_LIBRARIES").unwrap_or_else(|| "/mnt/MAIN_STORAGE/Libraries/Kontakt".into());
    let Some(path) = std::env::split_paths(&roots).map(|r| r.join(relative)).find(|p| p.is_file()) else { eprintln!("SKIP: missing keyswitched Afflatus library"); return; };
    let loaded = V2Loader.prepare(&LoadRequest { path, sample_rate: 48000., ..Default::default() }, &mut |_| {}, &|| false).unwrap();
    let inst = loaded.instrument.clone().unwrap();
    assert_eq!(inst.switching.owner, ir::SwitchOwner::Behavior);
    let ids = identities(&inst.articulations);
    let n = inst.articulations.iter().position(|a| !a.default && !a.switch_keys.is_empty()).unwrap();
    let original = inst.articulations[n].switch_keys[0];
    let remap = (0..128).find(|k| !inst.articulations.iter().any(|a| a.switch_keys.contains(k))).unwrap();
    let mut overlay = Overlay::default();
    overlay.set(&ids[n], Trigger::Keys(vec![remap]));
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, loaded.part);
    route(&mut core, &inst, &overlay);
    tap(&mut core, remap);
    assert_eq!(core.articulation(0), Some(n), "real script receives remapped source-key tap");
    core.select_articulation(0, inst.articulations.iter().position(|a| a.default).unwrap_or(0));
    core.render(128);
    tap(&mut core, original);
    assert_ne!(core.articulation(0), Some(n), "replaced original is suppressed");
    overlay.keep_originals = true;
    route(&mut core, &inst, &overlay);
    tap(&mut core, original);
    assert_eq!(core.articulation(0), Some(n));
    eprintln!("Afflatus: {} stable rows; remap {remap} → source {original}; clear/keep policy verified", inst.articulations.len());
}

#[test]
fn keyswitch_cell_modes_route_actual_source_values_and_user_overrides() {
    let (mut core, mut inst) = fixture(ir::SwitchOwner::Native);
    let ids = identities(&inst.articulations);
    inst.articulations[0].alternatives.velocities = Some(ir::VelocityRange { low: 19, high: 36 });
    inst.articulations[0].alternatives.controller = Some(ir::ControllerRange { controller: 12, low: 3, high: 3 });
    inst.articulations[0].alternatives.program = Some(90);
    for (driver, event) in [
        (ir::Driver::Velocity, Event::midi1(0x90, 60, 20)),
        (ir::Driver::Controller, Event::midi1(0xb0, 12, 3)),
        (ir::Driver::Program, Event::midi1(0xc0, 90, 0)),
    ] {
        let mut overlay = Overlay { driver: Some(driver as u8), ..Default::default() };
        assert!(core.select_articulation(0, 2));
        route(&mut core, &inst, &overlay);
        core.event(0, event);
        core.render(16);
        assert_eq!(core.articulation(0), Some(0), "actual source {driver:?}");
        if driver == ir::Driver::Program {
            overlay.set(&ids[0], Trigger::Program(Some(99)));
            route(&mut core, &inst, &overlay);
            core.select_articulation(0, 2);
            core.event(0, Event::midi1(0xc0, 99, 0));
            core.render(16);
            assert_eq!(core.articulation(0), Some(0), "edited program through production ingress");
            assert_eq!(core.problems(0).ignored_input, 0, "MIDI1 program selector consumes the message");
            core.select_articulation(0, 2);
            core.event(0, Event::Ump([0x40c0_0000, 99 << 24]));
            core.render(16);
            assert_eq!(core.articulation(0), Some(0), "MIDI2 program selector changes selection");
            assert_eq!(core.problems(0).ignored_input, 0, "MIDI2 program selector consumes the message");
        }
        core.event(0, Event::midi1(0x80, 60, 0));
        core.render(16);
    }
}

#[test]
fn keyswitch_program_messages_without_a_selector_are_counted() {
    let (mut core, inst) = fixture(ir::SwitchOwner::Native);
    route(&mut core, &inst, &Overlay::default());
    assert!(core.select_articulation(0, 2));
    core.event(0, Event::midi1(0xc0, 0, 0));
    assert_eq!(core.problems(0).ignored_input, 1);
    core.event(0, Event::Ump([0x40c0_0000, 0]));
    assert_eq!(core.problems(0).ignored_input, 2);
    core.render(16);
    assert_eq!(core.articulation(0), Some(2), "unhandled program changes leave selection unchanged");
}

#[test]
fn keyswitch_keyless_authored_row_routes_through_its_existing_control() {
    let mut inst = ir::Instrument::default();
    let control = sampler_ksp::derived_control_id(0, "$choice");
    inst.articulations = vec![ir::Articulation { source: "choice-axis".into(), name: "Choice".into(), control: Some(control.0), ..Default::default() }];
    inst.switching.owner = ir::SwitchOwner::Behavior;
    inst.behaviors.push(ir::Behavior { name: "choice".into(), language: ir::Language::Ksp, slot: Some(0), state: Vec::new(), requires: Vec::new(), source: "on init\n declare ui_button $choice\n declare ui_knob $readback (0,10,1)\nend on\non ui_control($choice)\n $readback := 7\nend on".into() });
    let loaded = sampler_kontakt::prepare(inst.clone(), vec![], &Default::default()).unwrap();
    let limits = limits(&loaded.plan).0;
    let mut part = Part::new(Runtime::new(loaded.plan, limits).unwrap(), MixTree::instrument("choice")).unwrap();
    part.set_drivers(&inst);
    part.articulations = Some(0);
    let mut core = V2Core::with_parts(1, 48000.);
    core.install(0, Some(Box::new(part)));
    let mut overlay = Overlay::default();
    overlay.set(&identities(&inst.articulations)[0], Trigger::Keys(vec![49]));
    route(&mut core, &inst, &overlay);
    tap(&mut core, 49);
    assert_eq!(core.control_value(0, sampler_ui_ir::ControlId(sampler_ksp::derived_control_id(0, "$readback").0)), Some(7.));
    assert_eq!(core.articulation(0), Some(0));
}
