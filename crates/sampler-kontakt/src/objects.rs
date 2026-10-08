//! Retain authored settings before snapshot/script writes or playback filtering.
use ni_file::kontakt::objects as ni;
use sampler_ir as ir;

pub(crate) fn program(version: u16, v: &ni::ProgramPublicParams) -> ir::kontakt::Program {
    ir::kontakt::Program {
        name: v.name.clone(),
        num_bytes_samples_total: v.num_bytes_samples_total,
        transpose: v.transpose,
        volume: v.volume,
        pan: v.pan,
        tune: v.tune,
        low_velocity: v.low_velocity,
        high_velocity: v.high_velocity,
        low_key: v.low_key,
        high_key: v.high_key,
        default_key_switch: v.default_key_switch,
        dfd_channel_preload_size: v.dfd_channel_preload_size,
        group_solo: v.group_solo,
        library_id: v.library_id,
        fingerprint: v.fingerprint,
        loading_flags: v.loading_flags,
        cat_icon_idx: v.cat_icon_idx,
        instrument_credits: v.instrument_credits.clone(),
        instrument_author: v.instrument_author.clone(),
        instrument_url: v.instrument_url.clone(),
        instrument_cat1: v.instrument_cat1,
        instrument_cat2: v.instrument_cat2,
        instrument_cat3: v.instrument_cat3,
        unknown_tail: v.unknown_tail.clone(),

        version,
    }
}

pub(crate) fn voice(v: &ni::VoiceLimit) -> ir::kontakt::VoiceLimit {
    ir::kontakt::VoiceLimit {
        name: v.name.clone(),
        kill_mode: v.kill_mode,
        prefer_released: v.prefer_released,
        max_num_voices: v.max_num_voices,
        ms_fade_time: v.ms_fade_time,
        exclusion_group: v.exclusion_group,
    }
}

pub(crate) fn group(g: &ni::Group, v: &ni::GroupParams) -> ir::kontakt::Group {
    let (source, source_error) = match g.source_params() {
        Ok((source, tail)) => (
            Some(ir::kontakt::Source {
                private_tail: tail.to_vec(),
                flag: g.source_identity().map_or(0, |v| v.flag),
                version: source.version,
                mode: source.mode,
                bytes: source.bytes,
                fields: source
                    .fields
                    .into_iter()
                    .map(|f| ir::kontakt::SourceField {
                        offset: f.offset,
                        name: f.name,
                        value: match f.value {
                            ni::SourceValue::Float(v) => ir::kontakt::SourceValue::Float(v),
                            ni::SourceValue::Integer(v) => ir::kontakt::SourceValue::Integer(v),
                            ni::SourceValue::Flag(v) => ir::kontakt::SourceValue::Flag(v),
                        },
                    })
                    .collect(),
            }),
            None,
        ),
        Err(e) => (None, Some(e.to_string())),
    };
    ir::kontakt::Group {
        name: v.name.clone(),
        volume: v.volume,
        pan: v.pan,
        tune: v.tune,
        key_tracking: v.key_tracking,
        reverse: v.reverse,
        release_trigger: v.release_trigger,
        release_trigger_note_monophonic: v.release_trigger_note_monophonic,
        rls_trig_counter: v.rls_trig_counter,
        midi_channel: v.midi_channel,
        voice_group_index: v.voice_group_index,
        fx_idx_amp_split_point: v.fx_idx_amp_split_point,
        muted: v.muted,
        soloed: v.soloed,
        interp_quality: v.interp_quality,
        unknown_tail: v.unknown_tail.clone(),
        version: g.0.version,
        criteria_mask: v.start_criteria.mask,
        criteria: v.start_criteria.items.iter().map(criterion).collect(),
        criteria_unknown_tail: v.start_criteria.unknown_tail.clone(),
        source,
        source_error,
    }
}

fn criterion(v: &ni::StartCriteriaParams) -> ir::kontakt::Criterion {
    ir::kontakt::Criterion {
        mode: v.mode,
        next_criteria: v.next_criteria,
        key_min: v.key_min,
        key_max: v.key_max,
        controller: v.controller,
        cc_min: v.cc_min,
        cc_max: v.cc_max,
        cycle_class: v.cycle_class,
        slice_zone_idx: v.slice_zone_idx,
        slice_zone_slice_idx: v.slice_zone_slice_idx,
        sequencer_only: v.sequencer_only,
    }
}

pub(crate) fn zone(
    version: u16,
    group: u32,
    v: ni::ZoneParams,
    loops: &ni::LoopArray,
) -> ir::kontakt::Zone {
    ir::kontakt::Zone {
        sample_start: v.sample_start,
        sample_end: v.sample_end,
        sample_start_mod_range: v.sample_start_mod_range,
        low_velocity: v.low_velocity,
        high_velocity: v.high_velocity,
        low_key: v.low_key,
        high_key: v.high_key,
        fade_low_velocity: v.fade_low_velocity,
        fade_high_velocity: v.fade_high_velocity,
        fade_low_key: v.fade_low_key,
        fade_high_key: v.fade_high_key,
        root_key: v.root_key,
        zone_volume: v.zone_volume,
        zone_pan: v.zone_pan,
        zone_tune: v.zone_tune,
        filename_prefix: v.filename_prefix,
        filename_id: v.filename_id,
        sample_data_type: v.sample_data_type,
        sample_rate: v.sample_rate,
        num_channels: v.num_channels,
        num_frames: v.num_frames,
        reserved1: v.reserved1,
        reserved2: v.reserved2,
        root_note: v.root_note,
        tuning: v.tuning,
        reserved3: v.reserved3,
        reserved4: v.reserved4,
        unknown_tail: v.unknown_tail,
        version,
        group,
        loops: loops
            .items
            .iter()
            .zip(&loops.slots)
            .map(|(v, slot)| loop_record(*slot, v))
            .collect(),
    }
}

fn loop_record(slot: u8, v: &ni::Loop) -> ir::kontakt::Loop {
    ir::kontakt::Loop {
        mode: v.mode,
        loop_start: v.loop_start,
        loop_length: v.loop_length,
        loop_count: v.loop_count,
        alternating_loop: v.alternating_loop,
        loop_tuning: v.loop_tuning,
        x_fade_length: v.x_fade_length,
        slot,
    }
}
