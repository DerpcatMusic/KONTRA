//! Authored regression coverage for the private Doctor core remediation.
use sampler_core::*;
mod support;

#[test]
fn wrong_control_domains_refuse_before_dsp_projection_without_heap_or_mutation() {
    let id = ControlId(7);
    let cases = [
        (
            ControlDomain::Integer {
                min: i64::MIN,
                max: i64::MAX,
            },
            ControlValue::Integer(0),
            [ControlValue::Real(0.), ControlValue::Toggle(false)],
        ),
        (
            ControlDomain::Real {
                min: -f64::MAX,
                max: f64::MAX,
            },
            ControlValue::Real(0.),
            [ControlValue::Integer(0), ControlValue::Toggle(false)],
        ),
        (
            ControlDomain::Toggle,
            ControlValue::Toggle(true),
            [ControlValue::Integer(1), ControlValue::Real(1.)],
        ),
    ];
    for (domain, default, wrong) in cases {
        for value in wrong {
            let invalid = Prepared::new(48_000, vec![], vec![], 0)
                .unwrap()
                .with_controls(vec![ControlDefinition {
                    id,
                    domain,
                    default: value,
                }]);
            assert!(matches!(invalid, Err(Error::InvalidInput)));
        }
        let plan = Prepared::new(48_000, vec![], vec![], 0)
            .unwrap()
            .with_controls(vec![ControlDefinition {
                id,
                domain,
                default,
            }])
            .unwrap()
            .with_voice_chains(
                vec![
                    VoiceChain::new(
                        vec![Processor::ControlGain(ControlRange {
                            control: id,
                            low: 0.,
                            high: 1.,
                            ramp_frames: 0,
                        })],
                        vec![],
                        0,
                    )
                    .unwrap(),
                ],
                vec![],
            )
            .unwrap();
        let limits = Limits::for_plan(&plan, 2, 2);
        let mut rt = Runtime::new(plan, limits).unwrap();
        let plan = rt.active_plan();
        support::without_heap(|| {
            for value in wrong {
                assert_eq!(
                    rt.edit_controls(plan, Some(0), &[ControlWrite { id, value }]),
                    Err(Error::InvalidInput)
                );
                assert_eq!(rt.control_value(plan, id), Ok(default));
                assert_eq!(rt.control_revision(plan), Ok(0));
                rt.render(&mut []).unwrap();
            }
        });
    }
}

#[test]
fn widget_rows_keep_the_complete_preflight_atomic_and_no_heap() {
    let widgets = [
        WidgetDefinition {
            id: ControlId(1),
            source_slot: 0,
            ui_id: 1,
            instance: ScriptInstanceId(0),
            storage: WidgetStorage::Cells {
                offset: 0,
                len: 2,
                real: false,
                min: 0.,
                max: 100.,
            },
            drop: None,
            menu: None,
            program: None,
            stage: 0,
        },
        WidgetDefinition {
            id: ControlId(2),
            source_slot: 0,
            ui_id: 2,
            instance: ScriptInstanceId(0),
            storage: WidgetStorage::Cells {
                offset: 2,
                len: 2,
                real: true,
                min: -1.,
                max: 1.,
            },
            drop: None,
            menu: None,
            program: None,
            stage: 0,
        },
    ];
    let prepared = Prepared::new(48_000, vec![], vec![], 0)
        .unwrap()
        .with_script_instances(vec![vec![0; 4]])
        .unwrap()
        .with_widgets(widgets.to_vec())
        .unwrap();
    let limits = Limits::for_plan(&prepared, 2, 0);
    let mut rt = Runtime::new(prepared, limits).unwrap();
    let plan = rt.active_plan();
    let context = ControlContext {
        performance: rt.performance(0).unwrap(),
        origin: ChannelAddress {
            protocol: Protocol::Native,
            port: 0,
            group: 0,
            channel: 0,
        },
        channels: 1,
    };
    let row = |id, index, value| WidgetEdit {
        id,
        index,
        value,
        interaction: WidgetInteraction::default(),
    };
    support::without_heap(|| {
        let integer = ControlId(1);
        let real = ControlId(2);
        for bad in [
            WidgetValue::Real(0.5),
            WidgetValue::Integer(101),
            WidgetValue::Text(Text::new("bad")),
        ] {
            let edits = [
                row(integer, 0, WidgetValue::Integer(20)),
                row(integer, 1, bad),
            ];
            assert_eq!(
                rt.invoke_widget(context, plan, Some(0), &edits),
                Err(Error::InvalidInput)
            );
            assert_eq!(
                rt.widget_value(plan, integer, 0),
                Ok(WidgetValue::Integer(0))
            );
            assert_eq!(rt.control_revision(plan), Ok(0));
        }
        assert_eq!(
            rt.invoke_widget(
                context,
                plan,
                Some(0),
                &[
                    row(integer, 0, WidgetValue::Integer(20)),
                    row(integer, 1, WidgetValue::Integer(70)),
                ]
            ),
            Ok((1, None))
        );
        assert_eq!(
            rt.widget_value(plan, integer, 0),
            Ok(WidgetValue::Integer(20))
        );
        assert_eq!(
            rt.widget_value(plan, integer, 1),
            Ok(WidgetValue::Integer(70))
        );
        for bad in [
            WidgetValue::Integer(1),
            WidgetValue::Real(f64::NAN),
            WidgetValue::Real(1.01),
        ] {
            let edits = [row(real, 0, WidgetValue::Real(-0.5)), row(real, 1, bad)];
            assert_eq!(
                rt.invoke_widget(context, plan, Some(1), &edits),
                Err(Error::InvalidInput)
            );
            assert_eq!(rt.widget_value(plan, real, 0), Ok(WidgetValue::Real(0.)));
            assert_eq!(rt.control_revision(plan), Ok(1));
        }
        assert_eq!(
            rt.invoke_widget(
                context,
                plan,
                Some(1),
                &[
                    row(real, 0, WidgetValue::Real(-0.5)),
                    row(real, 1, WidgetValue::Real(0.75)),
                ]
            ),
            Ok((2, None))
        );
        assert_eq!(rt.widget_value(plan, real, 0), Ok(WidgetValue::Real(-0.5)));
        assert_eq!(rt.widget_value(plan, real, 1), Ok(WidgetValue::Real(0.75)));
    });
}

#[test]
fn midi_set_forms_share_exact_edit_and_id_refusal_laws_without_heap() {
    let initial = MidiObject::new(vec![
        MidiObjectEvent {
            id: 3,
            position: 0,
            ..Default::default()
        },
        MidiObjectEvent {
            id: 4,
            position: 10,
            ..Default::default()
        },
    ])
    .unwrap();
    for parameter in [
        0,
        1,
        2,
        3,
        midi_par::CHANNEL,
        midi_par::COMMAND,
        midi_par::BYTE_ONE,
        midi_par::BYTE_TWO,
        midi_par::POSITION,
        midi_par::LENGTH,
        midi_par::TRACK,
        99,
    ] {
        let (mut short, mut explicit) = (initial.clone(), initial.clone());
        short.first(-1);
        explicit.first(-1);
        support::without_heap(|| {
            assert_eq!(
                short.apply(MidiAction::Set(parameter), &[7], None),
                explicit.apply(
                    MidiAction::SetParameter,
                    &[MIDI_CURRENT_EVENT, parameter, 7],
                    None
                )
            );
            for id in [3, 4] {
                assert_eq!(
                    short.apply(MidiAction::GetParameter, &[id, parameter], None),
                    explicit.apply(MidiAction::GetParameter, &[id, parameter], None)
                );
            }
            assert_eq!(short.selected_id(), explicit.selected_id());
        });
    }
    let mut midi = initial;
    midi.first(-1);
    support::without_heap(|| {
        assert_eq!(
            midi.apply(MidiAction::Set(midi_par::ID), &[4], None),
            Err(Error::InvalidInput)
        );
        assert_eq!(midi.selected_id(), 3);
        assert_eq!(
            midi.apply(
                MidiAction::SetParameter,
                &[MIDI_ALL_EVENTS, midi_par::ID, 8],
                None
            ),
            Err(Error::InvalidInput)
        );
        assert_eq!(midi.apply(MidiAction::Set(midi_par::ID), &[8], None), Ok(0));
        assert_eq!(midi.selected_id(), 8);
        assert_eq!(
            midi.apply(MidiAction::GetParameter, &[4, midi_par::POSITION], None),
            Ok(10)
        );
    });
}
