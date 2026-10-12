//! Port from v1 0cb7a8a0:src/cache.rs: compact numeric zone tables.
//! Typed extensions stay in a small JSON tuple; physical records retain every field.
use sampler_ir as ir;
struct Writer<'a>(&'a mut Vec<u8>);
impl Writer<'_> {
    fn u32(&mut self, x: u32) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn u64(&mut self, x: u64) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn bytes(&mut self, x: &[u8]) {
        self.u64(x.len() as u64);
        self.0.extend_from_slice(x);
    }
    fn u8(&mut self, x: u8) {
        self.0.push(x);
    }
    fn u16(&mut self, x: u16) {
        self.0.extend_from_slice(&x.to_le_bytes());
    }
    fn f32(&mut self, x: f32) {
        self.u32(x.to_bits());
    }
    fn f64(&mut self, x: f64) {
        self.u64(x.to_bits());
    }
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.take(N)?.try_into().ok()
    }
    fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }
    fn bytes(&mut self) -> Option<&'a [u8]> {
        let n = usize::try_from(self.u64()?).ok()?;
        self.take(n)
    }
    fn len(&mut self, size: usize) -> Option<usize> {
        let n = usize::try_from(self.u64()?).ok()?;
        (n <= self.0.len() / size).then_some(n)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_le_bytes)
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_bits(self.u32()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_bits(self.u64()?))
    }
    fn boolean(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn index(&mut self) -> Option<usize> {
        usize::try_from(self.u64()?).ok()
    }
}
fn optional(value: u64) -> Option<Option<usize>> {
    if value == u64::MAX {
        Some(None)
    } else {
        Some(Some(usize::try_from(value).ok()?))
    }
}
fn pitch(w: &mut Writer, value: ir::Pitch) {
    let (tag, v) = match value {
        ir::Pitch::Cents(v) => (0, v),
        ir::Pitch::Semitones(v) => (1, v),
        ir::Pitch::Ratio(v) => (2, v),
    };
    w.u8(tag);
    w.f64(v);
}
fn read_pitch(r: &mut Reader) -> Option<ir::Pitch> {
    let tag = r.u8()?;
    let v = r.f64()?;
    Some(match tag {
        0 => ir::Pitch::Cents(v),
        1 => ir::Pitch::Semitones(v),
        2 => ir::Pitch::Ratio(v),
        _ => return None,
    })
}
pub(crate) fn encode(
    zones: &[ir::Zone],
    physical: &[ir::kontakt::Zone],
    out: &mut Vec<u8>,
) -> serde_json::Result<()> {
    let mut w = Writer(out);
    w.u64(zones.len() as u64);
    for z in zones {
        w.u64(z.asset.0 as u64);
        w.u64(z.group.map_or(u64::MAX, |v| v.0 as u64));
        w.0.extend_from_slice(&[
            z.keys.low,
            z.keys.high,
            z.velocities.low,
            z.velocities.high,
            z.fades.velocity_in,
            z.fades.velocity_out,
            z.fades.key_in,
            z.fades.key_out,
        ]);
        pitch(&mut w, z.tune);
        let (tag, value) = match z.gain {
            ir::Gain::Linear(v) => (0, v),
            ir::Gain::Decibels(v) => (1, v),
        };
        w.u8(tag);
        w.f64(value);
        w.f64(z.pan.position);
        w.u8(match z.pan.law {
            ir::PanLaw::Balance => 0,
            ir::PanLaw::EqualPower => 1,
        });
        match z.pitch {
            ir::KeyTracking::Fixed => w.u8(0),
            ir::KeyTracking::Tracked { root } => {
                w.u8(1);
                w.u8(root);
            }
            ir::KeyTracking::Scaled {
                root,
                cents_per_key,
            } => {
                w.u8(2);
                w.u8(root);
                w.u32(cents_per_key as u32);
            }
        }
        match z.velocity {
            ir::VelocityResponse::None => w.u8(0),
            ir::VelocityResponse::Linear => w.u8(1),
            ir::VelocityResponse::Power(v) => {
                w.u8(2);
                w.f64(v);
            }
        }
        w.u64(z.playback.start);
        w.u8(z.playback.end.is_some() as u8);
        w.u64(z.playback.end.unwrap_or(0));
        w.u8(z.playback.reverse as u8);
        w.u64(z.playback.start_range);
        let extra = serde_json::to_vec(&(
            &z.conditions,
            z.trigger,
            z.selection,
            z.articulation,
            &z.axes,
            &z.playback.looping,
            z.chain,
            z.amplitude,
            &z.routes,
        ))?;
        w.bytes(&extra);
    }
    w.u64(physical.len() as u64);
    for z in physical {
        w.u16(z.version);
        w.u32(z.group);
        for v in [z.sample_start, z.sample_end, z.sample_start_mod_range] {
            w.u32(v as u32);
        }
        for v in [
            z.low_velocity,
            z.high_velocity,
            z.low_key,
            z.high_key,
            z.fade_low_velocity,
            z.fade_high_velocity,
            z.fade_low_key,
            z.fade_high_key,
            z.root_key,
        ] {
            w.u16(v as u16);
        }
        for v in [z.zone_volume, z.zone_pan, z.zone_tune] {
            w.f32(v);
        }
        if let Some(prefix) = z.filename_prefix {
            w.u8(1);
            w.0.extend_from_slice(&prefix);
        } else {
            w.u8(0);
        }
        for v in [z.filename_id, z.sample_data_type, z.sample_rate] {
            w.u32(v as u32);
        }
        w.u8(z.num_channels);
        for v in [z.num_frames, z.reserved1] {
            w.u32(v as u32);
        }
        if let Some(v) = z.reserved2 {
            w.u8(1);
            w.u32(v as u32);
        } else {
            w.u8(0);
        }
        w.u32(z.root_note as u32);
        w.f32(z.tuning);
        w.u8(z.reserved3);
        w.u32(z.reserved4 as u32);
        w.bytes(&z.unknown_tail);
        w.u64(z.loops.len() as u64);
        for l in &z.loops {
            w.u8(l.slot);
            for v in [l.mode, l.loop_start, l.loop_length, l.loop_count] {
                w.u32(v as u32);
            }
            w.u8(l.alternating_loop as u8);
            w.f32(l.loop_tuning);
            w.u32(l.x_fade_length as u32);
        }
    }
    Ok(())
}
pub(crate) fn decode(bytes: &[u8]) -> Option<(Vec<ir::Zone>, Vec<ir::kontakt::Zone>)> {
    let mut r = Reader(bytes);
    let len = r.len(64)?;
    let mut zones = Vec::with_capacity(len);
    for _ in 0..len {
        let asset = ir::AssetRef(r.index()?);
        let group = optional(r.u64()?)?.map(ir::GroupRef);
        let fields = r.array::<8>()?;
        let tune = read_pitch(&mut r)?;
        let tag = r.u8()?;
        let value = r.f64()?;
        let gain = match tag {
            0 => ir::Gain::Linear(value),
            1 => ir::Gain::Decibels(value),
            _ => return None,
        };
        let position = r.f64()?;
        let law = match r.u8()? {
            0 => ir::PanLaw::Balance,
            1 => ir::PanLaw::EqualPower,
            _ => return None,
        };
        let pitch = match r.u8()? {
            0 => ir::KeyTracking::Fixed,
            1 => ir::KeyTracking::Tracked { root: r.u8()? },
            2 => ir::KeyTracking::Scaled {
                root: r.u8()?,
                cents_per_key: r.u32()? as i32,
            },
            _ => return None,
        };
        let velocity = match r.u8()? {
            0 => ir::VelocityResponse::None,
            1 => ir::VelocityResponse::Linear,
            2 => ir::VelocityResponse::Power(r.f64()?),
            _ => return None,
        };
        let start = r.u64()?;
        let has_end = r.boolean()?;
        let end_value = r.u64()?;
        let end = has_end.then_some(end_value);
        let reverse = r.boolean()?;
        let start_range = r.u64()?;
        let (conditions, trigger, selection, articulation, axes, looping, chain, amplitude, routes) =
            serde_json::from_slice(r.bytes()?).ok()?;
        zones.push(ir::Zone {
            asset,
            group,
            keys: ir::KeyRange {
                low: fields[0],
                high: fields[1],
            },
            velocities: ir::VelocityRange {
                low: fields[2],
                high: fields[3],
            },
            fades: ir::Fades {
                velocity_in: fields[4],
                velocity_out: fields[5],
                key_in: fields[6],
                key_out: fields[7],
            },
            tune,
            gain,
            pan: ir::Pan { position, law },
            pitch,
            velocity,
            playback: ir::Playback {
                start,
                end,
                reverse,
                start_range,
                looping,
            },
            conditions,
            trigger,
            selection,
            articulation,
            axes,
            chain,
            amplitude,
            routes,
        });
    }
    let len = r.len(64)?;
    let mut physical = Vec::with_capacity(len);
    for _ in 0..len {
        let version = r.u16()?;
        let group = r.u32()?;
        let sample_start = r.u32()? as i32;
        let sample_end = r.u32()? as i32;
        let sample_start_mod_range = r.u32()? as i32;
        let low_velocity = r.u16()? as i16;
        let high_velocity = r.u16()? as i16;
        let low_key = r.u16()? as i16;
        let high_key = r.u16()? as i16;
        let fade_low_velocity = r.u16()? as i16;
        let fade_high_velocity = r.u16()? as i16;
        let fade_low_key = r.u16()? as i16;
        let fade_high_key = r.u16()? as i16;
        let root_key = r.u16()? as i16;
        let zone_volume = r.f32()?;
        let zone_pan = r.f32()?;
        let zone_tune = r.f32()?;
        let filename_prefix = if r.boolean()? {
            Some(r.array::<6>()?)
        } else {
            None
        };
        let filename_id = r.u32()? as i32;
        let sample_data_type = r.u32()? as i32;
        let sample_rate = r.u32()? as i32;
        let num_channels = r.u8()?;
        let num_frames = r.u32()? as i32;
        let reserved1 = r.u32()? as i32;
        let reserved2 = if r.boolean()? {
            Some(r.u32()? as i32)
        } else {
            None
        };
        let root_note = r.u32()? as i32;
        let tuning = r.f32()?;
        let reserved3 = r.u8()?;
        let reserved4 = r.u32()? as i32;
        let unknown_tail = r.bytes()?.to_vec();
        let len = r.len(26)?;
        let mut loops = Vec::with_capacity(len);
        for _ in 0..len {
            loops.push(ir::kontakt::Loop {
                slot: r.u8()?,
                mode: r.u32()? as i32,
                loop_start: r.u32()? as i32,
                loop_length: r.u32()? as i32,
                loop_count: r.u32()? as i32,
                alternating_loop: r.boolean()?,
                loop_tuning: r.f32()?,
                x_fade_length: r.u32()? as i32,
            });
        }
        physical.push(ir::kontakt::Zone {
            version,
            group,
            sample_start,
            sample_end,
            sample_start_mod_range,
            low_velocity,
            high_velocity,
            low_key,
            high_key,
            fade_low_velocity,
            fade_high_velocity,
            fade_low_key,
            fade_high_key,
            root_key,
            zone_volume,
            zone_pan,
            zone_tune,
            filename_prefix,
            filename_id,
            sample_data_type,
            sample_rate,
            num_channels,
            num_frames,
            reserved1,
            reserved2,
            root_note,
            tuning,
            reserved3,
            reserved4,
            unknown_tail,
            loops,
        });
    }
    r.0.is_empty().then_some((zones, physical))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zone_tables_roundtrip_and_reject_truncation_counts_and_tags() {
        let mut a = ir::Zone::new(ir::AssetRef(17));
        a.pitch = ir::KeyTracking::Scaled {
            root: 43,
            cents_per_key: -27,
        };
        a.tune = ir::Pitch::Ratio(1.2345);
        a.gain = ir::Gain::Decibels(-7.2);
        a.pan = ir::Pan {
            position: -0.4,
            law: ir::PanLaw::EqualPower,
        };
        a.velocity = ir::VelocityResponse::Power(2.3);
        a.trigger = ir::Trigger::Transition { low: -7, high: 2 };
        a.group = Some(ir::GroupRef(4));
        a.playback.start = 123;
        a.playback.end = Some(456);
        a.playback.reverse = true;
        a.playback.start_range = 7;
        let zones = [a, ir::Zone::new(ir::AssetRef(1))];
        let mut bytes = Vec::new();
        encode(&zones, &[], &mut bytes).unwrap();
        assert_eq!(decode(&bytes).unwrap().0, zones);
        assert!(bytes.len() < serde_json::to_vec(&zones).unwrap().len() / 2);
        assert!(decode(&bytes[..bytes.len() - 1]).is_none());
        let mut bad = bytes.clone();
        bad[..8].copy_from_slice(&u64::MAX.to_le_bytes());
        assert!(decode(&bad).is_none());
    }
    #[test]
    fn physical_zone_record_preserves_all_versioned_metadata_and_loops() {
        let record = ir::kontakt::Zone {
            version: 7,
            group: 23,
            sample_start: -1,
            sample_end: -2,
            sample_start_mod_range: 3,
            low_velocity: 4,
            high_velocity: 5,
            low_key: 6,
            high_key: 7,
            fade_low_velocity: 8,
            fade_high_velocity: 9,
            fade_low_key: 10,
            fade_high_key: 11,
            root_key: 12,
            zone_volume: 0.75,
            zone_pan: -0.25,
            zone_tune: 1.5,
            filename_prefix: Some([1, 2, 3, 4, 5, 6]),
            filename_id: 13,
            sample_data_type: 14,
            sample_rate: 48000,
            num_channels: 2,
            num_frames: 15,
            reserved1: 16,
            reserved2: Some(-17),
            root_note: 18,
            tuning: -0.5,
            reserved3: 19,
            reserved4: 20,
            unknown_tail: vec![21, 22],
            loops: vec![ir::kontakt::Loop {
                slot: 2,
                mode: 3,
                loop_start: 4,
                loop_length: 5,
                loop_count: 6,
                alternating_loop: true,
                loop_tuning: 0.125,
                x_fade_length: 7,
            }],
        };
        let mut other = record.clone();
        other.filename_prefix = None;
        other.reserved2 = None;
        other.loops.clear();
        let physical = [record, other];
        let mut bytes = Vec::new();
        encode(&[], &physical, &mut bytes).unwrap();
        assert_eq!(decode(&bytes).unwrap().1, physical);
        let mut bad = bytes;
        bad.push(0);
        assert!(decode(&bad).is_none());
    }
}
