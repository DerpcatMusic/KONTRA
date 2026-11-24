use std::io::Cursor;

use crate::{kontakt::structured_object::StructuredObject, read_bytes::ReadBytesExt, Error};

#[derive(Debug)]
pub struct Zone(pub StructuredObject);

/// Type:           StructuredObject
/// Kontakt 7:      BZone, BProgram::readZones()
/// KontaktIO:      K4PL_Zone<K4PO::K4PL_ZoneDataV95>
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ZoneParams {
    pub sample_start: i32,
    pub sample_end: i32,
    pub sample_start_mod_range: i32,
    pub low_velocity: i16,
    pub high_velocity: i16,
    pub low_key: i16,
    pub high_key: i16,
    pub fade_low_velocity: i16,
    pub fade_high_velocity: i16,
    pub fade_low_key: i16,
    pub fade_high_key: i16,
    pub root_key: i16,
    pub zone_volume: f32,
    pub zone_pan: f32,
    pub zone_tune: f32,
    /// Whether the native record contains sample metadata. Absent metadata
    /// below uses zero placeholders and filename_id -1; it must never resolve PCM.
    pub sample_present: bool,
    /// The index of the file in the filetable.
    pub filename_id: i32,
    pub filename_prefix: Option<[u8; 6]>,
    pub sample_data_type: i32,
    pub sample_rate: i32,
    pub num_channels: u8,
    pub num_frames: i32,
    pub reserved1: i32,
    pub reserved2: Option<i32>,
    pub root_note: i32,
    pub tuning: f32,
    pub reserved3: u8,
    pub reserved4: i32,
    pub unknown_tail: Vec<u8>,
    // LoopArray 0x39
    // QuickBrowseData 0x4e
    // PrivateRawObject 0x35
}

impl Zone {
    /// Sample reference from the common zone prefix, independent of later
    /// version-specific public parameters.
    pub fn filename_id(&self) -> Result<i32, Error> {
        if !self.has_sample()? {
            return Ok(-1);
        }
        let at = if self.0.version >= 0x9a { 48 } else { 42 };
        let bytes: [u8; 4] = self
            .0
            .public_data
            .get(at..at + 4)
            .and_then(|s| s.try_into().ok())
            .ok_or(Error::Static("Truncated zone sample reference"))?;
        Ok(i32::from_le_bytes(bytes))
    }

    /// The v0x9a..9c common prefix ends with two flags and an opaque u32.
    /// The second flag gates the entire sample suffix in the native serializer.
    pub fn has_sample(&self) -> Result<bool, Error> {
        if !matches!(self.0.version, 0x92..=0x95 | 0x97..=0x9c) {
            return Err(Error::Static("Unsupported zone version"));
        }
        if self.0.version < 0x9a {
            return Ok(true);
        }
        let prefix = self
            .0
            .public_data
            .get(42..48)
            .ok_or(Error::Static("Truncated zone sample-presence prefix"))?;
        match prefix.get(1) {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            Some(_) => Err(Error::Static("Invalid zone sample-presence flag")),
            None => Err(Error::Static("Truncated zone sample-presence flag")),
        }
    }

    pub fn read<R: ReadBytesExt>(mut reader: R) -> Result<Self, Error> {
        Ok(Self(StructuredObject::read(&mut reader)?))
    }

    pub fn params(&self) -> Result<ZoneParams, Error> {
        let mut reader = Cursor::new(&self.0.public_data);

        let sample_present = self.has_sample()?;
        let mut params = ZoneParams {
            sample_start: reader.read_i32_le()?,
            sample_end: reader.read_i32_le()?,
            sample_start_mod_range: reader.read_i32_le()?,
            low_velocity: reader.read_i16_le()?,
            high_velocity: reader.read_i16_le()?,
            low_key: reader.read_i16_le()?,
            high_key: reader.read_i16_le()?,
            fade_low_velocity: reader.read_i16_le()?,
            fade_high_velocity: reader.read_i16_le()?,
            fade_low_key: reader.read_i16_le()?,
            fade_high_key: reader.read_i16_le()?,
            root_key: reader.read_i16_le()?,
            zone_volume: reader.read_f32_le()?,
            zone_pan: reader.read_f32_le()?,
            zone_tune: reader.read_f32_le()?,
            filename_prefix: if self.0.version >= 0x9a {
                let mut prefix = [0; 6];
                std::io::Read::read_exact(&mut reader, &mut prefix)?;
                Some(prefix)
            } else {
                None
            },
            sample_present,
            filename_id: -1,
            ..ZoneParams::default()
        };
        if sample_present {
            params.filename_id = reader.read_i32_le()?;
            params.sample_data_type = reader.read_i32_le()?;
            params.sample_rate = reader.read_i32_le()?;
            params.num_channels = reader.read_u8()?;
            params.num_frames = reader.read_i32_le()?;
            params.reserved1 = reader.read_i32_le()?;
            params.reserved2 = if self.0.version < 0x96 {
                Some(reader.read_i32_le()?)
            } else {
                None
            };
            params.root_note = reader.read_i32_le()?;
            params.tuning = reader.read_f32_le()?;
            params.reserved3 = reader.read_u8()?;
            params.reserved4 = reader.read_i32_le()?;
        }
        params.unknown_tail = reader.read_all()?;
        Ok(params)
    }

    /// Per-zone modulation envelopes (`BZoneLevelEnv`, Kontakt 8.13.1
    /// [ref] -> [ref] -> [ref]), stored inside
    /// `private_data` at byte offset 56 as a 16-slot bitmask followed by each
    /// present envelope record.
    pub fn zone_envelopes(&self) -> Result<Vec<ZoneEnvelope>, Error> {
        if self.0.version == 0x96 || !(0x92..=0x9c).contains(&self.0.version) {
            return Err(Error::Static("Unsupported zone version for zone envelopes"));
        }
        let private = &self.0.private_data;
        const PREFIX_LEN: usize = 56;
        let tail_len = match self.0.version {
            0x92 => 4,
            0x93..=0x94 => 12,
            0x95 | 0x97 | 0x98 => 25,
            0x99..=0x9c => 29,
            _ => unreachable!(),
        };
        if private.len() < PREFIX_LEN + 2 + tail_len {
            return Err(Error::Static("Truncated zone private data"));
        }
        let mut reader = Cursor::new(&private[PREFIX_LEN..]);
        let mask_lo = reader.read_u8()?;
        let mask_hi = reader.read_u8()?;
        let mask = u16::from_le_bytes([mask_lo, mask_hi]);
        let mut envelopes = Vec::new();
        for slot in 0u8..16 {
            if mask & (1u16 << slot) == 0 {
                continue;
            }
            // BZoneLevelEnv::read header: chunk type byte (0) and version (1).
            let kind = reader.read_u8()?;
            if kind != 0 {
                return Err(Error::Generic(format!(
                    "Unexpected zone envelope type byte {kind}"
                )));
            }
            let version = reader.read_u16_le()?;
            if version != 1 {
                return Err(Error::Generic(format!(
                    "Unsupported zone envelope version {version}"
                )));
            }
            let present = reader.read_u8()? != 0;
            if !present {
                continue;
            }
            let ext_mod_idx = reader.read_i16_le()?;
            let loop_start = reader.read_i32_le()?;
            let loop_length = reader.read_i32_le()?;
            let loop_active = reader.read_u8()? != 0;
            let speed_lock = reader.read_u8()? != 0;
            let target_len = reader.read_u32_le()? as usize;
            if target_len > 256 {
                return Err(Error::Static("Zone envelope target name too long"));
            }
            let target_bytes = reader.read_bytes(target_len)?;
            let target = String::from_utf8_lossy(&target_bytes).into_owned();
            let target_idx = reader.read_u8()?;
            let count = reader.read_i16_le()?;
            if count < 0 || count > 32_000 {
                return Err(Error::Generic(format!(
                    "Invalid zone envelope point count {count}"
                )));
            }
            let mut points: Vec<ZoneEnvelopePoint> = Vec::with_capacity(count as usize);
            for _ in 0..count {
                let mut x = reader.read_i32_le()?;
                let y = reader.read_f32_le()?;
                let curve = reader.read_f32_le()?;
                if !y.is_finite() || !curve.is_finite() {
                    return Err(Error::Static("Non-finite zone envelope point"));
                }
                // Kontakt [ref] enforces a minimum 32-frame spacing.
                if let Some(prev) = points.last() {
                    let min_x = prev.x.saturating_add(32);
                    if x < min_x {
                        x = min_x;
                    }
                }
                points.push(ZoneEnvelopePoint { x, y, curve });
            }
            envelopes.push(ZoneEnvelope {
                slot,
                ext_mod_idx,
                loop_start,
                loop_length,
                loop_active,
                speed_lock,
                target,
                target_idx,
                points,
            });
        }
        let remaining = private.len() - PREFIX_LEN - reader.position() as usize;
        if remaining != tail_len {
            return Err(Error::Generic(format!(
                "Zone private tail length mismatch: expected {tail_len}, got {remaining}"
            )));
        }
        Ok(envelopes)
    }
}

/// One per-zone envelope (`BZoneLevelEnv`) attached to a zone.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ZoneEnvelope {
    /// Slot in the zone's 16-entry envelope array (`0..16`).
    pub slot: u8,
    /// 0-based external modulator index in the owning group (`ExternalModArray32`),
    /// matching `BZEnvMod` (`BParExternalMod` category 2).
    pub ext_mod_idx: i16,
    /// Loop start in sample frames.
    pub loop_start: i32,
    /// Loop length in sample frames.
    pub loop_length: i32,
    pub loop_active: bool,
    /// When true, envelope advances with the voice's sample playback rate
    /// (pitch and sample-rate ratio); when false, at 1 frame per host sample.
    pub speed_lock: bool,
    pub target: String,
    pub target_idx: u8,
    pub points: Vec<ZoneEnvelopePoint>,
}

/// One breakpoint of a [`ZoneEnvelope`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ZoneEnvelopePoint {
    /// Position in sample frames.
    pub x: i32,
    /// Bipolar output value (`-1.0..=1.0` in Kontakt).
    pub y: f32,
    /// Segment curvature (`-1.0..=1.0`, `0.0` linear).
    pub curve: f32,
}

impl Zone {
    /// Source-defined KSP zone identity, independent of ZoneList position.
    ///
    /// Original Kontakt8 0fe6356e: version dispatch [ref] picks a private
    /// reader by version (`0x92..=0x9c`, `0x96` throws) and every branch ends
    /// in the same tail at [ref]: one `read_u32` stored to
    /// `(zone+0x10)+0xa4`. `0x99..=0x9c` additionally read an aux count just
    /// before it (`+0x5c`); `0x92..=0x98` do not. So the identity is the
    /// terminal i32 of the private at every admitted version, whatever
    /// variable-length sub-records precede it.
    /// That is why the saved privates differ in length (83, 87, 140..176) yet
    /// all end with the same field; byte 56 is layout noise, not a flag.
    ///
    /// The private's first field is a u32 read into `+0xa8`, then [ref]
    /// reads the mode at `+4` and throws unless it is 0, 1 or 2. The public
    /// six-byte prefix contains different user/sample/owner fields. No ordinal
    /// is fabricated: a private that does not decode gives an error, not an id.
    pub fn source_id(&self) -> Result<Option<u32>, Error> {
        if self.0.version == 0x96 || !(0x92..=0x9c).contains(&self.0.version) {
            return Err(Error::Static("Unverified zone source identity version"));
        }
        let private = &self.0.private_data;
        if private.len() < 8 {
            return Err(Error::Static(
                "Unverified zone source identity private layout",
            ));
        }
        // cf0360 accepts only these three values before the auxiliary mask.
        let mode_bytes: [u8; 4] = private[4..8]
            .try_into()
            .map_err(|_| Error::Static("Unverified zone source identity private layout"))?;
        if u32::from_le_bytes(mode_bytes) > 2 {
            return Err(Error::Static("Invalid zone private mode"));
        }
        let n = private.len();
        let id_bytes: [u8; 4] = private[n - 4..n]
            .try_into()
            .map_err(|_| Error::Static("Unverified zone source identity private layout"))?;
        match i32::from_le_bytes(id_bytes) {
            -1 => Ok(None),
            id if id >= 0 => Ok(Some(id as u32)),
            _ => Err(Error::Static("Invalid zone source identity")),
        }
    }
}

#[cfg(test)]
mod source_identity_tests {
    use super::*;
    fn zone(id: i32) -> Zone {
        let mut private_data = vec![0; 87];
        private_data[83..87].copy_from_slice(&id.to_le_bytes());
        let mut public_data = vec![0; 48];
        public_data[42..48].copy_from_slice(&[0, 1, 255, 255, 255, 255]);
        Zone(StructuredObject {
            version: 0x9c,
            private_data,
            public_data,
            children: Vec::new(),
        })
    }
    #[test]
    fn explicit_zero_and_sparse_native_ids_do_not_use_public_owner_or_ordinal() {
        for id in [0, 7, 2018] {
            assert_eq!(zone(id).source_id().unwrap(), Some(id as u32));
        }
        assert_eq!(zone(-1).source_id().unwrap(), None);
        assert!(zone(-2).source_id().is_err());
    }
    #[test]
    fn every_private_length_takes_its_terminal_i32_as_identity() {
        // The native reader's last read is the identity at every admitted
        // version, so variable-length privates (83, 140..176) keep the id last.
        for len in [12, 83, 87, 88, 140, 152, 176, 512] {
            let mut z = zone(7);
            z.0.private_data.resize(len, 0);
            let n = len;
            z.0.private_data[n - 4..].copy_from_slice(&7i32.to_le_bytes());
            assert_eq!(z.source_id().unwrap(), Some(7), "private length {len}");
        }
        for len in [0, 4] {
            let mut z = zone(7);
            z.0.private_data.resize(len, 0);
            assert!(z.source_id().is_err(), "private length {len}");
        }
    }
    #[test]
    fn version_branches_that_share_the_identity_tail_are_admitted_and_0x96_is_not() {
        for version in 0x92..=0x9c {
            let mut z = zone(7);
            z.0.version = version;
            if version == 0x96 {
                assert!(z.source_id().is_err());
            } else {
                assert_eq!(z.source_id().unwrap(), Some(7));
            }
        }
        let mut high = zone(7);
        high.0.version = 0x9d;
        assert!(high.source_id().is_err());
    }
    #[test]
    fn bad_mode_and_negative_ids_are_errors_not_identities() {
        let mut z = zone(7);
        z.0.private_data[4] = 3;
        assert!(z.source_id().is_err());
        z.0.private_data[4] = 0;
        let n = z.0.private_data.len();
        z.0.private_data[n - 4..].copy_from_slice(&(-2i32).to_le_bytes());
        assert!(z.source_id().is_err());
    }
}

#[cfg(test)]
mod zone_envelope_tests {
    use super::*;

    fn push_env(
        buf: &mut Vec<u8>,
        present: bool,
        ext_mod_idx: i16,
        loop_start: i32,
        loop_length: i32,
        loop_active: bool,
        speed_lock: bool,
        target: &str,
        target_idx: u8,
        points: &[(i32, f32, f32)],
    ) {
        buf.push(0); // kind == 0
        buf.extend_from_slice(&1u16.to_le_bytes()); // version == 1
        buf.push(u8::from(present));
        if !present {
            return;
        }
        buf.extend_from_slice(&ext_mod_idx.to_le_bytes());
        buf.extend_from_slice(&loop_start.to_le_bytes());
        buf.extend_from_slice(&loop_length.to_le_bytes());
        buf.push(u8::from(loop_active));
        buf.push(u8::from(speed_lock));
        buf.extend_from_slice(&(target.len() as u32).to_le_bytes());
        buf.extend_from_slice(target.as_bytes());
        buf.push(target_idx);
        buf.extend_from_slice(&(points.len() as i16).to_le_bytes());
        for &(x, y, curve) in points {
            buf.extend_from_slice(&x.to_le_bytes());
            buf.extend_from_slice(&y.to_le_bytes());
            buf.extend_from_slice(&curve.to_le_bytes());
        }
    }

    fn make_zone(version: u16, mask: u16, body: &[u8]) -> Zone {
        let tail_len = match version {
            0x92 => 4,
            0x93..=0x94 => 12,
            0x95 | 0x97 | 0x98 => 25,
            0x99..=0x9c => 29,
            _ => 29,
        };
        let mut private_data = vec![0u8; 56];
        private_data.extend_from_slice(&mask.to_le_bytes());
        private_data.extend_from_slice(body);
        private_data.extend(std::iter::repeat_n(0u8, tail_len));
        Zone(StructuredObject {
            version,
            private_data,
            public_data: vec![0; 48],
            children: Vec::new(),
        })
    }

    #[test]
    fn empty_bitmask_parses_across_all_admitted_versions() {
        for version in 0x92..=0x9c {
            let z = make_zone(version, 0, &[]);
            if version == 0x96 {
                assert!(z.zone_envelopes().is_err());
            } else {
                assert!(
                    z.zone_envelopes().unwrap().is_empty(),
                    "version {version:#x}"
                );
            }
        }
    }

    #[test]
    fn sparse_slots_flags_and_min_spacing_decode_cleanly() {
        let mut body = Vec::new();
        // Slot 2: absent marker (present == 0)
        push_env(&mut body, false, 0, 0, 0, false, false, "", 0, &[]);
        // Slot 5: active envelope with points closer than 32 frames
        push_env(
            &mut body,
            true,
            4,
            1000,
            2048,
            true,
            true,
            "volume",
            0,
            &[(0, -1.0, 0.25), (10, 0.0, 0.0), (5000, 1.0, -0.5)],
        );
        let mask = (1u16 << 2) | (1u16 << 5);
        let envs = make_zone(0x9c, mask, &body).zone_envelopes().unwrap();
        assert_eq!(envs.len(), 1);
        let e = &envs[0];
        assert_eq!(e.slot, 5);
        assert_eq!(e.ext_mod_idx, 4);
        assert_eq!((e.loop_start, e.loop_length), (1000, 2048));
        assert!(e.loop_active && e.speed_lock);
        assert_eq!(e.target, "volume");
        assert_eq!(e.points.len(), 3);
        assert_eq!(e.points[0].x, 0);
        // Enforced 32-frame minimum spacing from previous point (0 -> 32).
        assert_eq!(e.points[1].x, 32);
        assert_eq!(e.points[2].x, 5000);
    }

    #[test]
    fn malformed_headers_and_tail_mismatches_return_errors() {
        // Bad chunk kind byte
        let mut bad_kind = Vec::new();
        push_env(&mut bad_kind, false, 0, 0, 0, false, false, "", 0, &[]);
        bad_kind[0] = 7;
        assert!(make_zone(0x9c, 1, &bad_kind).zone_envelopes().is_err());

        // Bad version word
        let mut bad_ver = Vec::new();
        push_env(&mut bad_ver, false, 0, 0, 0, false, false, "", 0, &[]);
        bad_ver[1] = 2;
        assert!(make_zone(0x9c, 1, &bad_ver).zone_envelopes().is_err());

        // Trailing garbage changes tail length
        let mut z = make_zone(0x9c, 0, &[]);
        z.0.private_data.push(0);
        assert!(z.zone_envelopes().is_err());
    }

    #[test]
    fn non_finite_point_values_are_rejected() {
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for (y, curve) in [(invalid, 0.0), (0.0, invalid)] {
                let mut body = Vec::new();
                push_env(
                    &mut body,
                    true,
                    0,
                    0,
                    0,
                    false,
                    false,
                    "",
                    0,
                    &[(0, y, curve)],
                );
                assert!(make_zone(0x9c, 1, &body).zone_envelopes().is_err());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;
    use crate::Error;

    #[test]
    #[ignore = "needs vendor/ni-file/test-data, which is not in the repository"]
    fn test_zone_data_v9a_000() -> Result<(), Error> {
        let file =
            File::open("tests/data/Objects/Kontakt/ZoneData/ZoneDataV9A/ZoneDataV9A-000.kon")?;
        let zone = Zone::read(file)?;
        assert_eq!(zone.0.version, 0x9A);
        zone.params()?;
        Ok(())
    }
}
