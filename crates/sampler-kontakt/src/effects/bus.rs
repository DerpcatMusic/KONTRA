//! Instrument buses: the groups routed to each bus with its fader and pan,
//! and the Send Levels that feed the instrument's send slots.
use sampler_ir::Reason;


use super::{BUS, Chain, Convolution, Decoded, ImpulseRequest, Impulses, Lowering, Note, Notes, Scope, SendTap, Site, Slot, IDENTITY, chain_with, product};
use ni_file::kontakt::objects::{InsertBus, Program};

/// An instrument bus (`$NI_BUS_OFFSET` + `index`) with the groups routed to it.
pub(crate) struct BusPlan {
    pub index: usize,
    /// The bus fader (linear) and pan.
    pub volume: f32,
    pub pan: f32,
    pub groups: Vec<sampler_ir::GroupRef>,
}

/// The program's instrument buses with the groups routed to them (`routes`
/// pairs a group with its 0-based bus).
pub(crate) fn bus_plans(program: &Program, routes: &[(sampler_ir::GroupRef, u8)]) -> Vec<BusPlan> {
    program
        .0
        .children
        .iter()
        .filter(|child| child.id == BUS)
        .enumerate()
        .filter_map(|(index, child)| {
            let params = InsertBus::try_from(child).ok()?.params().ok()?;
            Some(BusPlan {
                index,
                volume: params.volume,
                pan: params.pan,
                groups: routes
                    .iter()
                    .filter(|&&(_, bus)| usize::from(bus) == index)
                    .map(|&(group, _)| group)
                    .collect(),
            })
        })
        .collect()
}

/// The instrument-level racks as buses: every group feeds an insert bus
/// (the insert rack), whose Send Levels slots feed one bus per send slot
/// (the send rack's effects), and the main rack follows both. A group routed
/// to an instrument bus (`buses`) feeds that bus instead, whose fader and rack
/// run before the insert bus. Changes nothing when the racks do nothing.
pub(crate) fn instrument_buses(
    ir: &mut sampler_ir::Instrument,
    racks: &[(String, Vec<Slot>)],
    buses: &[BusPlan],
    dynamic: bool,
    load: &mut dyn FnMut(i32) -> Result<Decoded, String>,
) -> (Vec<(String, Note)>, Vec<(usize, sampler_ir::BusRef)>) {
    let mut indexed = |request: ImpulseRequest| load(request.index);
    instrument_buses_with_recipes(ir, racks, buses, dynamic, &mut indexed, None)
}

pub(crate) fn instrument_buses_with_recipes(
    ir: &mut sampler_ir::Instrument,
    racks: &[(String, Vec<Slot>)],
    buses: &[BusPlan],
    dynamic: bool,
    load: &mut dyn FnMut(ImpulseRequest) -> Result<Decoded, String>,
    recipes: Option<&mut Vec<(ImpulseRequest, Convolution)>>,
) -> (Vec<(String, Note)>, Vec<(usize, sampler_ir::BusRef)>) {
    use sampler_ir::{BusRef, ChainRef, Output, Scope as IrScope, Send, SendPosition};
    let rack = |name: &str| {
        racks
            .iter()
            .find(|(n, _)| n == name)
            .map_or(&[][..], |(_, s)| s.as_slice())
    };
    let mut report = Vec::new();
    let mut take = |name: &str, c: &Chain| {
        report.extend(c.notes.iter().map(|n| (name.to_string(), n.clone())));
    };
    let mut store = std::mem::take(&mut ir.impulses);
    let mut source = Impulses {
        store: &mut store,
        recipes,
        load,
    };
    // `$NI_INSERT_BUS` 1, `$NI_SEND_BUS` 0, `$NI_MAIN_BUS` 2.
    let generic = |n| dynamic.then_some((-1, n));
    let insert = chain_with(
        rack("instrument insert"),
        Scope::Bus,
        Some(&mut source),
        generic(1),
        (-1, 1),
        &[],
    );
    take("instrument insert", &insert);
    let main = chain_with(
        rack("instrument main"),
        Scope::Bus,
        Some(&mut source),
        generic(2),
        (-1, 2),
        &[],
    );
    take("instrument main", &main);
    // A send slot's effect runs on its own bus, fed at the Send Levels slot's level.
    let mut sends = Vec::new();
    for slot in rack("instrument send")
        .iter()
        .filter(|s| dynamic || !s.bypass)
    {
        let mut c = chain_with(
            std::slice::from_ref(slot),
            Scope::Bus,
            Some(&mut source),
            generic(0),
            (-1, 0),
            &[],
        );
        // Port from v1 0cb7a8a0:src/fx/processor.rs (process returns and
        // SendInputs::tap): bypassed returns contribute no dry signal.
        // Keep the real slot control so scripts can re-enable the return.
        if dynamic && !c.processors.is_empty() {
            c.processors.push(sampler_ir::Processor::SendReturnGate {
                address: sampler_ir::SlotAddress {
                    group: -1,
                    slot: slot.slot as i32,
                    generic: 0,
                },
            });
        }
        take("instrument send", &c);
        let level = insert.sends.get(slot.slot).copied().unwrap_or(1.0);
        if !c.processors.is_empty() {
            sends.push((slot.slot, c.processors, f64::from(level)));
        }
    }
    // Instrument buses: only the ones a group feeds and that do something.
    // `$NI_BUS_OFFSET` + the bus number.
    let mut instrument = Vec::new();
    let mut invalid_pan = Vec::new();
    for bus in buses.iter().filter(|b| dynamic || !b.groups.is_empty()) {
        let name = format!("bus {}", bus.index);
        let c = chain_with(
            rack(&name),
            Scope::Bus,
            Some(&mut source),
            generic(1000 + bus.index as i32),
            (-1, 1000 + bus.index as i32),
            &[],
        );
        take(&name, &c);
        let mut processors = c.processors;
        if dynamic {
            // A script sets the bus volume and routes groups to any bus at run
            // time, so every bus exists and its volume is a Mix block's wet level.
            processors.push(sampler_ir::Processor::Mix {
                count: 1,
                address: sampler_ir::SlotAddress {
                    group: -1,
                    slot: sampler_core::BUS_VOLUME_SLOT,
                    generic: 1000 + bus.index as i32,
                },
                dry: 0.0,
                wet: f64::from(bus.volume),
                bypass: false,
            });
            processors.push(sampler_ir::Processor::StereoMatrix(IDENTITY));
        }
        if bus.pan.is_finite() {
            if bus.pan != 0. {
                // NI Bus Insert Effects amplifier: BInsertBus audio 0x140824910
                // calls 0x140823d70 with channel flags 0/1: independent linear balance.
                let left = 1. - bus.pan.clamp(0., 1.);
                let right = (bus.pan + 1.).clamp(0., 1.);
                processors.push(sampler_ir::Processor::StereoMatrix([
                    [f64::from(left), 0.], [0., f64::from(right)],
                ]));
            }
        } else {
            invalid_pan.push((name, (0, "instrument bus pan".into(), bus.pan.to_string(),
                sampler_ir::Reason::InvalidValue)));
        }
        if dynamic || !processors.is_empty() || (bus.volume - 1.0).abs() > 1e-4 {
            instrument.push((bus, processors));
        }
    }
    report.extend(invalid_pan);
    ir.impulses = store;
    let chained = !(insert.processors.is_empty() && sends.is_empty() && main.processors.is_empty());
    if !chained && instrument.is_empty() {
        ir.input_bus = None;
        return (report, Vec::new());
    }
    // Bus order: insert, sends, main, then the instrument buses.
    let main_bus = (!main.processors.is_empty()).then_some(sends.len() + 1);
    let target = main_bus.map_or(Output::Master, |i| Output::Bus(BusRef(i)));
    let add = |ir: &mut sampler_ir::Instrument,
               name: String,
               processors: Vec<sampler_ir::Processor>,
               sends,
               output,
               gain| {
        let chain = (!processors.is_empty()).then(|| {
            let index = ir.buses.len();
            ir.chains.push(sampler_ir::Chain {
                scope: IrScope::Bus(BusRef(index)),
                pre_amplitude: processors,
                post_amplitude: Vec::new(),
            });
            ChainRef(ir.chains.len() - 1)
        });
        ir.buses.push(sampler_ir::Bus {
            name,
            chain,
            sends,
            output,
            gain,
        });
    };
    let unity = sampler_ir::Gain::UNITY;
    let feeds: Vec<_> = sends
        .iter()
        .enumerate()
        .map(|(i, (_, _, level))| Send {
            to: Output::Bus(BusRef(i + 1)),
            gain: sampler_ir::Gain::Linear(*level),
            position: SendPosition::PostChain,
        })
        .collect();
    let send_buses: Vec<_> = sends
        .iter()
        .enumerate()
        .map(|(i, (slot, _, _))| (*slot, BusRef(i + 1)))
        .collect();
    let entry = if chained {
        let mut segments = Vec::new();
        let mut cursor = 0;
        for tap in &insert.send_taps {
            let n = tap.position;
            let feeds = if tap.bypass {
                Vec::new()
            } else {
                send_buses
                    .iter()
                    .filter_map(|&(slot, bus)| {
                        tap.levels.get(slot).map(|&level| Send {
                            to: Output::Bus(bus),
                            gain: sampler_ir::Gain::Linear(f64::from(level)),
                            position: SendPosition::PostChain,
                        })
                    })
                    .collect()
            };
            segments.push((insert.processors[cursor..n].to_vec(), feeds));
            cursor = n;
        }
        if segments.is_empty() {
            segments.push((insert.processors, feeds));
        } else if cursor < insert.processors.len() {
            segments.push((insert.processors[cursor..].to_vec(), Vec::new()));
        }
        // Each continuation is another summed bus, so its history and send
        // position use the existing preallocated bus DAG.
        let continuation = sends.len() + 1 + usize::from(main_bus.is_some());
        let mut segments = segments.into_iter().enumerate().peekable();
        let (_, (processors, feeds)) = segments.next().expect("insert segment");
        let next = if segments.peek().is_some() {
            Output::Bus(BusRef(continuation))
        } else {
            target
        };
        add(ir, "insert".into(), processors, feeds, next, unity);
        for (slot, processors, _) in sends {
            add(
                ir,
                format!("send {slot}"),
                processors,
                Vec::new(),
                target,
                unity,
            );
        }
        if main_bus.is_some() {
            add(
                ir,
                "main".into(),
                main.processors,
                Vec::new(),
                Output::Master,
                unity,
            );
        }
        while let Some((i, (processors, feeds))) = segments.next() {
            let next = if segments.peek().is_some() {
                Output::Bus(BusRef(continuation + i))
            } else {
                target
            };
            add(
                ir,
                format!("insert continuation {i}"),
                processors,
                feeds,
                next,
                unity,
            );
        }
        for group in &mut ir.groups {
            group.output = Output::Bus(BusRef(0));
        }
        Output::Bus(BusRef(0))
    } else {
        Output::Master
    };
    ir.input_bus = if let Output::Bus(bus) = entry {
        Some(bus)
    } else {
        None
    };
    for (bus, processors) in instrument {
        let at = BusRef(ir.buses.len());
        add(
            ir,
            format!("bus {}", bus.index),
            processors,
            Vec::new(),
            entry,
            if dynamic {
                unity
            } else {
                sampler_ir::Gain::Linear(f64::from(bus.volume))
            },
        );
        ir.bus_addresses.push((1000 + bus.index as i32, at));
        for group in &bus.groups {
            ir.groups[group.0].output = Output::Bus(at);
        }
    }
    (report, send_buses)
}


/// Send Levels (`Params::SendLevels`): linear levels into each instrument send
/// slot. They feed buses, not the signal, so the tap records where it sits.
pub(super) fn slot_send_levels(b: &mut Lowering, s: &Site, notes: &mut Notes, sends: &[f32]) -> bool {
    b.flush();
    let sends: Vec<_> = sends
        .iter()
        .map(|&level| {
            if level.is_finite() && level >= 0.0 {
                level
            } else {
                notes.push(("send level".into(), "non-finite or negative".into(), Reason::InvalidValue));
                0.0
            }
        })
        .collect();
    if s.scope == Scope::Bus && b.out.sends.is_empty() {
        b.out.sends = sends.clone();
    }
    b.out.send_taps.push(SendTap {
        slot: s.fx.slot,
        position: b.out.processors.len(),
        levels: sends,
        bypass: s.fx.bypass,
    });
    b.combined = product(s.gain, b.combined);
    true
}

#[cfg(test)]
mod tests {
    use crate::effects::*;
    use crate::effects::test_support::*;

    #[test]
    fn instrument_bus_pan_only_uses_native_stereo_balance() {
        use sampler_ir as ir;
        for (pan, expected) in [(-1., [1., 0.]), (-0.5, [1., 0.5]),
            (0., [1., 1.]), (0.5, [0.5, 1.]), (1., [0., 1.])] {
            let mut instrument = ir::Instrument::default();
            instrument.groups.push(ir::Group::default());
            let (notes, _) = crate::effects::instrument_buses(&mut instrument, &[],
                &[crate::effects::BusPlan { index: 0, volume: 1., pan,
                    groups: vec![ir::GroupRef(0)] }], false, &mut |_| Err("unused".into()));
            assert!(notes.is_empty());
            if pan == 0. { continue; }
            assert_eq!(instrument.buses.len(), 1, "pan-only bus must exist");
            let chain = instrument.buses[0].chain.unwrap();
            let output = fx_probe(instrument.chains[chain.0].pre_amplitude.clone(),
                120., &[[1., 1.]]);
            assert_eq!(output[0], expected);
        }
    }

    /// Output's double-buffered layer buses (ANALOG STRINGS `switch_bus`): a
    /// bus saved at volume 0 sounds once the script writes its fader.
    #[test]
    fn script_bus_volume_writes_reach_the_dynamic_bus_fader() {
        use sampler_ir as ir;
        let mut instrument = ir::Instrument::default();
        instrument.groups.push(ir::Group::default());
        instrument.assets.push(ir::Asset { residency: Default::default(), location: ir::AssetLocation::Path("probe".into()), encoding: ir::Encoding::Wav, root_key: None, loops: vec![] });
        let mut zone = ir::Zone::new(ir::AssetRef(0));
        zone.keys = ir::KeyRange { low: 60, high: 60 };
        zone.group = Some(ir::GroupRef(0));
        zone.velocity = ir::VelocityResponse::None;
        instrument.zones.push(zone);
        let (notes, _) = crate::effects::instrument_buses(&mut instrument, &[],
            &[crate::effects::BusPlan { index: 0, volume: 0., pan: 0., groups: vec![ir::GroupRef(0)] }],
            true, &mut |_| Err("unused".into()));
        assert!(notes.is_empty(), "{notes:?}");
        let pcm = sampler_core::Pcm::new(48000, vec![[0.5; 2]; 4096].into_boxed_slice()).unwrap();
        let plan = sampler_core::lower::lower(&instrument, 48000, vec![pcm], |_, p| Ok(p)).unwrap();
        let limits = sampler_core::Limits::for_plan(&plan, 4, 4);
        let mut rt = sampler_core::Runtime::new(plan, limits).unwrap();
        rt.trigger(sampler_core::Input { protocol: sampler_core::Protocol::Native, port: 0, group: 0, channel: 0, key: 60, external_id: None }, 60, 1.0).unwrap();
        let mut out = [[0.0; 2]; 16];
        rt.render(&mut out).unwrap();
        assert_eq!(out[8], [0., 0.], "saved volume 0 mutes the bus");
        let address = sampler_core::EngineParameterAddress {
            parameter: sampler_core::engine_parameter_id("ENGINE_PAR_VOLUME").unwrap(),
            group: -1, slot: -1, generic: 1000,
        };
        rt.set_engine_parameter(address, 629_961).expect("bus fader write is bound");
        assert_eq!(rt.engine_parameter(address).unwrap(), 629_961, "read back through the same law");
        let mut ramp = vec![[0.0; 2]; 1024]; // past the slot's 10 ms ramp
        rt.render(&mut ramp).unwrap();
        assert!((ramp[1023][0] - 0.5).abs() < 1e-3, "0 dB fader passes the voice: {:?}", ramp[1023]);
    }

    #[test]
    fn send_rack_reverb_becomes_a_send_bus_fed_by_send_levels() {
        use sampler_ir as ir;
        let mut levels = 2u32.to_le_bytes().to_vec();
        levels.extend([0.5f32, 1.0].iter().flat_map(|x| x.to_le_bytes()));
        levels.extend(0u32.to_le_bytes());
        let mut reverb = Vec::new();
        for x in [0.0f32, 0.5, 0.5, 0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0] {
            reverb.extend(x.to_le_bytes());
        }
        let mut instrument = ir::Instrument {
            groups: vec![ir::Group {
                muted: false,
                wavetable: None,
                time_stretch: None,
                start: Vec::new(),
                name: "g".into(),
                gain: ir::Gain::UNITY,
                pan: ir::Pan::default(),
                tune: ir::Pitch::Semitones(0.0),
                chain: None,
                output: ir::Output::Master,
                voice_limit: None,
                source_voice_limit: None,
                monophonic_release: false,
                sends: Vec::new(),
                tap: None,
            }],
            ..Default::default()
        };
        let racks = vec![
            (
                "instrument insert".to_string(),
                vec![slot(0x17, levels, 1.0)],
            ),
            ("instrument send".to_string(), vec![slot(0x59, reverb, 1.0)]),
        ];
        let report = instrument_buses(&mut instrument, &racks, &[], false, &mut |_| {
            Err("none".into())
        });
        assert_eq!(instrument.buses.len(), 2, "{report:?}");
        assert_eq!(instrument.groups[0].output, ir::Output::Bus(ir::BusRef(0)));
        let feed = &instrument.buses[0].sends[0];
        assert_eq!(feed.to, ir::Output::Bus(ir::BusRef(1)));
        assert_eq!(feed.gain, ir::Gain::Linear(0.5));
        let chain = &instrument.chains[instrument.buses[1].chain.unwrap().0];
        assert!(matches!(
            chain.pre_amplitude[..],
            [ir::Processor::Space(_)]
        ));
        instrument.validate().unwrap();
        // A physical return at slot 3 remains addressable even when the
        // instrument level is zero; a group tap can still feed that return.
        let mut sparse = ir::Instrument::default();
        let mut return_slot = slot(0x59, racks[1].1[0].public.clone(), 1.0);
        return_slot.slot = 3;
        let mut zero = 4u32.to_le_bytes().to_vec();
        zero.extend([0.0f32; 4].iter().flat_map(|x| x.to_le_bytes()));
        zero.extend(0u32.to_le_bytes());
        let (_, returns) = instrument_buses(
            &mut sparse,
            &[
                ("instrument insert".into(), vec![slot(0x17, zero, 1.0)]),
                ("instrument send".into(), vec![return_slot]),
            ],
            &[],
            false,
            &mut |_| Err("none".into()),
        );
        assert_eq!(returns, vec![(3, ir::BusRef(1))]);
        assert_eq!(sparse.buses[0].sends[0].gain, ir::Gain::Linear(0.0));
        // Nothing to do: no buses.
        let mut plain = ir::Instrument::default();
        instrument_buses(&mut plain, &[], &[], false, &mut |_| Err("none".into()));
        assert!(plain.buses.is_empty());
        // A routed bus at half volume owns its groups' output; an unrouted one
        // is not built.
        let mut routed = instrument.clone();
        routed.buses.clear();
        routed.chains.clear();
        let plans = [
            BusPlan {
                index: 0,
                volume: 0.5,
                pan: 0.0,
                groups: vec![ir::GroupRef(0)],
            },
            BusPlan {
                index: 1,
                volume: 0.5,
                pan: 0.0,
                groups: Vec::new(),
            },
        ];
        instrument_buses(&mut routed, &[], &plans, false, &mut |_| Err("none".into()));
        assert_eq!(routed.buses.len(), 1);
        assert_eq!(routed.buses[0].gain, ir::Gain::Linear(0.5));
        assert_eq!(routed.buses[0].output, ir::Output::Master);
        assert_eq!(routed.groups[0].output, ir::Output::Bus(ir::BusRef(0)));
        routed.validate().unwrap();
    }

    #[test]
    fn voice_send_levels_are_admitted_on_both_sides_of_the_amplifier() {
        let mut levels = 1u32.to_le_bytes().to_vec();
        levels.extend(0.5f32.to_le_bytes());
        levels.extend(0u32.to_le_bytes());
        let mut before = slot(0x17, levels.clone(), 1.0);
        before.slot = 2;
        let mut after = slot(0x17, levels, 1.0);
        after.slot = 6;
        let (chain, boundary) = voice_chain(&[before, after], 4, None, 0, &[]);
        assert!(chain.notes.is_empty(), "{:?}", chain.notes);
        assert_eq!(boundary, 0);
        assert_eq!(chain.send_taps.len(), 2);
        assert_eq!(chain.send_taps[0].slot, 2);
        assert_eq!(
            chain.send_taps[0].position,
            sampler_ir::VoiceSendPosition::BeforeAmplitude(0)
        );
        assert_eq!(chain.send_taps[1].slot, 6);
        assert_eq!(
            chain.send_taps[1].position,
            sampler_ir::VoiceSendPosition::AfterAmplitude(0)
        );
    }

    #[test]
    fn instrument_send_tap_precedes_later_inserts_on_the_summed_bus() {
        use sampler_ir as ir;
        let mut before = slot(0x13, 2.0f32.to_le_bytes().to_vec(), 1.0);
        before.dry_level = 0.0;
        let mut levels = 1u32.to_le_bytes().to_vec();
        levels.extend(0.5f32.to_le_bytes());
        levels.extend(0u32.to_le_bytes());
        let mut tap = slot(0x17, levels, 1.0);
        tap.slot = 2;
        let mut after = slot(0x13, 3.0f32.to_le_bytes().to_vec(), 1.0);
        after.slot = 3;
        after.dry_level = 0.0;
        let mut ir = ir::Instrument::default();
        instrument_buses(
            &mut ir,
            &[
                ("instrument insert".into(), vec![before, tap, after]),
                (
                    "instrument send".into(),
                    vec![slot(0x13, 0.5f32.to_le_bytes().to_vec(), 1.0)],
                ),
            ],
            &[],
            false,
            &mut |_| Err("none".into()),
        );
        assert_eq!(ir.buses.len(), 3);
        assert_eq!(ir.buses[0].output, ir::Output::Bus(ir::BusRef(2)));
        assert_eq!(ir.buses[0].sends[0].to, ir::Output::Bus(ir::BusRef(1)));
        assert_eq!(ir.buses[0].sends[0].gain, ir::Gain::Linear(0.5));
        for (bus, gain) in [(0, 2.0), (2, 3.0)] {
            let stages = &ir.chains[ir.buses[bus].chain.unwrap().0].pre_amplitude;
            assert!(
                matches!(stages[..], [ir::Processor::Color(c)] if c.gainer.is_some_and(|x| x.gain == ir::Gain::Linear(gain) && x.dry == 0.0))
            );
        }
        ir.validate().unwrap();
    }
}
