//! Authored FX/modulation state retained in the IR, including unmodeled fields.
use ni_file::kontakt::{StructuredObject, objects::*};
use sampler_ir::{
    SourceParameter as Field, SourceParameterRecord as Record, SourceParameterValue as Value,
};

fn field(name: &'static str, value: Value) -> Field {
    Field { name, value }
}
fn number(name: &'static str, value: f32) -> Field {
    field(name, Value::Number(f64::from(value)))
}
fn integer(name: &'static str, value: impl Into<i64>) -> Field {
    field(name, Value::Integer(value.into()))
}
fn opaque(name: &'static str, value: &[u8]) -> Field {
    field(name, Value::Opaque(value.to_vec()))
}
fn numbers(name: &'static str, value: &[f32]) -> Field {
    field(
        name,
        Value::Numbers(value.iter().copied().map(f64::from).collect()),
    )
}

fn timing(record: EnvelopeTimingRecord) -> Vec<Field> {
    vec![
        number("unknown_0", record.values[0]),
        number("unknown_1", record.values[1]),
        number("unknown_2", record.values[2]),
        integer("unknown_flag", record.flag),
    ]
}
fn record(location: &str, id: u16, version: u16, fields: Vec<Field>) -> Record {
    Record {
        location: location.into(),
        object_id: id,
        version,
        fields,
    }
}

fn target(target: &ModTarget) -> Vec<Field> {
    let mut out = vec![
        field("parameter", Value::Text(target.param.clone())),
        field("name", Value::Text(target.name.clone())),
        number("magnitude", target.intensity),
        number("signed_depth", target.signed_intensity()),
        integer("smoothing_ms", target.lag_ms),
        field("invert", Value::Boolean(target.invert)),
        integer("unknown_i16", target.unknown_i16),
        integer("flags", target.unknown_flags),
        integer("module_slot", target.slot.map_or(-1, i64::from)),
    ];
    if let Some(shaper) = &target.shaper {
        out.push(field("shaper_enabled", Value::Boolean(shaper.enabled)));
        match &shaper.curve {
            ShaperCurve::Table(table) => out.push(numbers("shaper_table", table)),
            ShaperCurve::Breakpoints(points) => out.push(field(
                "shaper_points",
                Value::Records(
                    points
                        .iter()
                        .map(|p| vec![number("x", p.x), number("y", p.y), number("curve", p.curve)])
                        .collect(),
                ),
            )),
        }
    }
    out
}

pub(crate) fn internal(
    location: &str,
    object: &InternalMod,
    params: &InternalModParams,
) -> Result<Vec<Record>, ni_file::Error> {
    let mut records = vec![record(
        location,
        0x0d,
        object.0.version,
        vec![
            field("name", Value::Text(params.name.clone())),
            field(
                "targets",
                Value::Records(params.targets.iter().map(target).collect()),
            ),
            integer("routers_open", params.unknown_flags[0]),
            integer("bypass", params.unknown_flags[1]),
            integer("retrigger", params.unknown_flags[2]),
            integer("unknown_flag", params.unknown_flags[3]),
            integer("unknown_id", params.unknown_id),
        ],
    )];
    let source = object
        .0
        .children
        .first()
        .ok_or(ni_file::Error::Static("Missing internal source"))?;
    let envelope;
    let source = if source.id == 7 {
        envelope = StructuredObject::try_from(source)?;
        records.push(record(
            &format!("{location}/envelope"),
            7,
            envelope.version,
            vec![
                opaque("public", &envelope.public_data),
                opaque("private", &envelope.private_data),
            ],
        ));
        envelope
            .children
            .first()
            .ok_or(ni_file::Error::Static("Missing envelope source"))?
    } else {
        source
    };
    let object = StructuredObject::try_from(source)?;
    let fields = match &params.modulator {
        Modulator::Ahdsr(e) => vec![
            number("attack_curve", e.attack_curve),
            number("attack_ms", e.attack_ms),
            number("decay_ms", e.decay_ms),
            number("hold_ms", e.hold_ms),
            number("release_ms", e.release_ms),
            number("sustain_linear", e.sustain),
            integer("ahd_only", e.unknown_flag),
            field(
                "timing_records",
                Value::Records(e.timing_records()?.into_iter().map(timing).collect()),
            ),
            opaque("unknown_extension", &e.unknown_tail[52..]),
        ],
        Modulator::Flex(e) => vec![
            integer("sustain_index", e.sustain),
            integer("unknown_index", e.unknown_index),
            field(
                "points",
                Value::Records(
                    e.points
                        .iter()
                        .map(|p| {
                            vec![
                                number("delta_ms", p.time_ms),
                                number("level", p.level),
                                number("curve", p.curve),
                            ]
                        })
                        .collect(),
                ),
            ),
            field(
                "timing_record",
                Value::Records(vec![timing(e.timing_record()?)]),
            ),
            integer(
                "unknown_version_word",
                e.unknown_version_word().map_or(-1, i64::from),
            ),
        ],
        Modulator::Lfo(l) => {
            let mut fields = vec![
                integer("waveform", l.waveform),
                field("structured", Value::Boolean(l.structured)),
                number("fade_in_ms_or_count", l.initial_values[0]),
                number("rate_hz_or_count", l.initial_values[1]),
                number("pulse_width", l.initial_values[2]),
                number("phase_cycles", l.initial_values[3]),
                field("normalize_multi", Value::Boolean(l.records[0].flag)),
                number("frequency_note_value", l.records[0].values[0]),
                number("frequency_sync_unknown_1", l.records[0].values[1]),
                number("frequency_sync_unknown_2", l.records[0].values[2]),
                field("frequency_sync_flag", Value::Boolean(l.records[1].flag)),
                number("fade_note_value", l.records[1].values[0]),
                number("fade_sync_unknown_1", l.records[1].values[1]),
                number("fade_sync_unknown_2", l.records[1].values[2]),
                field("fade_sync_flag", Value::Boolean(l.trailing_flag)),
            ];
            if let Some(weights) = l.trailing_values {
                fields.push(numbers("multi_weights", &weights));
            }
            if let Some(flag) = l.additional_flag {
                fields.push(field("unknown_version_flag", Value::Boolean(flag)));
            }
            fields
        }
        Modulator::Other { .. } => vec![opaque("undecoded_object", &source.data)],
    };
    records.push(record(
        &format!("{location}/source"),
        source.id,
        object.version,
        fields,
    ));
    Ok(records)
}

pub(crate) fn external(location: &str, object: &ExternalMod, params: &ExternalModParams) -> Record {
    // Numeric source identity and payload stay distinct; no source enum renumbering.
    let (code, payload) = match params.source {
        ModSource::PitchBend => (1, 0),
        ModSource::PolyAftertouch => (2, 0),
        ModSource::MonoAftertouch => (3, 0),
        ModSource::MidiCc(cc) => (4, i64::from(cc)),
        ModSource::KeyPosition => (5, 0),
        ModSource::Velocity => (6, 0),
        ModSource::ReleaseVelocity => (7, 0),
        ModSource::ReleaseTriggerCounter => (8, 0),
        ModSource::Constant => (9, 0),
        ModSource::RandomUnipolar => (10, 0),
        ModSource::RandomBipolar => (11, 0),
        ModSource::Script(id) => (12, i64::from(id)),
        ModSource::Unassigned => (0, 0),
    };
    record(
        location,
        0x0c,
        object.0.version,
        vec![
            field("name", Value::Text(params.name.clone())),
            integer("source", code),
            integer("source_payload", payload),
            field(
                "targets",
                Value::Records(params.targets.iter().map(target).collect()),
            ),
            integer("unknown_id", params.unknown_id),
            opaque("unknown_source_data", &params.unknown_source_data),
            opaque("unknown_tail", &params.unknown_tail),
        ],
    )
}

pub(crate) fn rack(
    location: &str,
    array: &BParamArrayBParFX8,
) -> Result<Vec<Record>, ni_file::Error> {
    let mut out = Vec::new();
    for (slot, chunk) in array
        .items
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.as_ref().map(|c| (i, c)))
    {
        let fx = BParFX::try_from(chunk)?;
        let state = fx.params()?;
        let location = format!("{location}/slot {slot}");
        out.push(record(
            &location,
            0x25,
            fx.0.version,
            vec![
                integer("effect_type", state.effect_type),
                field("bypass", Value::Boolean(state.bypass)),
                number("output_linear", state.output_gain),
                number("dry_linear", state.dry_level),
                integer("reserved", state.reserved),
                integer("unknown_flag", state.unknown_flag),
                integer("unknown_id", state.unknown_id),
                opaque("public", &fx.0.public_data),
            ],
        ));
        let module = fx
            .effect()
            .ok_or(ni_file::Error::Static("Missing FX module"))?;
        let object = StructuredObject::try_from(module)?;
        let mut fields =
            match EffectParameters::read(module.id, object.version, &object.public_data) {
                Ok(Some(p)) => p
                    .fields
                    .into_iter()
                    .map(|f| {
                        field(
                            f.name,
                            match f.value {
                                EffectValue::Float(v) => Value::Number(f64::from(v)),
                                EffectValue::Integer(v) => Value::Integer(i64::from(v)),
                                EffectValue::Byte(v) => Value::Integer(i64::from(v)),
                                EffectValue::Floats(v) => {
                                    Value::Numbers(v.into_iter().map(f64::from).collect())
                                }
                            },
                        )
                    })
                    .collect(),
                // Unknown layouts stay intact in memory and are reported by chain translation.
                _ => vec![opaque("undecoded_public", &object.public_data)],
            };
        if !object.private_data.is_empty() {
            fields.push(opaque("undecoded_private", &object.private_data));
        }
        for child in &object.children {
            fields.push(opaque("undecoded_child", &child.data));
        }
        out.push(record(
            &format!("{location}/module"),
            module.id,
            object.version,
            fields,
        ));
    }
    Ok(out)
}

pub(crate) fn program(program: &Program) -> Result<Vec<Record>, ni_file::Error> {
    let mut out = Vec::new();
    let (mut racks, mut buses) = (0, 0);
    for chunk in &program.0.children {
        match chunk.id {
            0x3a => {
                let at = format!("instrument rack {racks}");
                racks += 1;
                out.extend(rack(&at, &BParamArrayBParFX8::try_from(chunk)?)?);
            }
            0x45 => {
                let bus = InsertBus::try_from(chunk)?;
                let params = bus.params()?;
                let at = format!("bus {buses}");
                buses += 1;
                out.push(record(
                    &at,
                    0x45,
                    bus.0.version,
                    vec![
                        field("name", Value::Text(params.name)),
                        number("volume_linear", params.volume),
                        number("pan", params.pan),
                        integer("output", params.output),
                        opaque("private", &bus.0.private_data),
                    ],
                ));
                if let Some(chunk) = bus.0.find_first(0x3a) {
                    out.extend(rack(&at, &BParamArrayBParFX8::try_from(chunk)?)?);
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

pub(crate) fn muted_group(group: &Group, index: usize) -> Result<Vec<Record>, ni_file::Error> {
    let mut out = Vec::new();
    for chunk in &group.0.children {
        match chunk.id {
            0x3b => {
                for (slot, m) in InternalModArray16::try_from(chunk)?.slots()? {
                    out.extend(internal(
                        &format!("group {index} internal slot {slot}"),
                        &m,
                        &m.params()?,
                    )?);
                }
            }
            0x3c => {
                for (slot, m) in ExternalModArray32::try_from(chunk)?.slots()? {
                    out.push(external(
                        &format!("group {index} external slot {slot}"),
                        &m,
                        &m.params()?,
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ni_file::kontakt::Chunk;
    #[test]
    fn bypassed_unmodeled_fx_keeps_physical_slot_and_all_storage_fields() {
        let module = Chunk {
            id: 0x20,
            data: {
                let mut b = vec![0, 0x50, 0];
                for v in [8f32, 22050.0, 0.25] {
                    b.extend(v.to_le_bytes());
                }
                b.push(1);
                b.extend(0.75f32.to_le_bytes());
                b
            },
        };
        let mut private = 10u32.to_le_bytes().to_vec();
        private.extend([0, 0, 0, 0, 0, 1]);
        private.extend(0.5f32.to_le_bytes());
        private.extend(1f32.to_le_bytes());
        private.extend((-1i32).to_le_bytes());
        let mut data = vec![1, 0x50, 0];
        data.extend((private.len() as u32).to_le_bytes());
        data.extend(private);
        data.extend([0; 4]);
        let mut children = Vec::new();
        module.write(&mut children).unwrap();
        data.extend((children.len() as u32).to_le_bytes());
        data.extend(children);
        let mut items: Vec<_> = (0..8).map(|_| None).collect();
        items[7] = Some(Chunk { id: 0x25, data });
        let records = rack(
            "group 3 insert",
            &BParamArrayBParFX8 {
                version: 0x12,
                items,
            },
        )
        .unwrap();
        assert_eq!(records.len(), 2);
        assert!(records[0].location.ends_with("slot 7"));
        assert_eq!(records[0].fields[1].value, Value::Boolean(true));
        assert_eq!(records[1].fields[0].value, Value::Number(8.0));
        assert_eq!(records[1].fields[1].value, Value::Number(22050.0));
        assert_eq!(records[1].fields[3].value, Value::Integer(1));
        assert_eq!(records[1].fields[4].value, Value::Number(0.75));
    }
}
