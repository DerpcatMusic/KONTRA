//! WAV, AIFF and FLAC to stereo frames, bounded as v1's `src/uvi/sample.rs`.

use sampler_kontakt::Decoded;
use std::io::Cursor;
use symphonia::core::{
    audio::SampleBuffer,
    codecs::DecoderOptions,
    errors::Error as AudioError,
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::{Limit, MetadataOptions},
    probe::Hint,
};

const MEMORY_LIMIT: usize = 512 << 20;

/// Interleaved samples, their channel count and rate.
pub(crate) fn decode_channels(bytes: &[u8]) -> Result<(Vec<f32>, usize, u32), String> {
    decode_channels_with_limit(bytes,MEMORY_LIMIT)
}
fn decode_channels_with_limit(bytes:&[u8],decoded_byte_limit:usize)->Result<(Vec<f32>,usize,u32),String> {
    if bytes.is_empty() || bytes.len() > MEMORY_LIMIT {
        return Err("audio is empty or exceeds 512 MiB".into());
    }
    let mut encoded = bytes.to_vec();
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 {
        // Symphonia needs a consistent RIFF extent; banks do not always keep one.
        encoded[4..8].copy_from_slice(&((bytes.len() - 8) as u32).to_le_bytes());
    }
    let stream = MediaSourceStream::new(Box::new(Cursor::new(encoded)), Default::default());
    let metadata = MetadataOptions {
        limit_metadata_bytes: Limit::Maximum(2 << 20),
        limit_visual_bytes: Limit::Maximum(2 << 20),
    };
    let mut format = symphonia::default::get_probe()
        .format(&Hint::new(), stream, &FormatOptions::default(), &metadata)
        .map_err(|e| format!("not WAV, AIFF or FLAC: {e}"))?
        .format;
    let track = format.default_track().ok_or("no audio track")?;
    let (id, params) = (track.id, track.codec_params.clone());
    let rate = params.sample_rate.ok_or("undeclared sample rate")?;
    let channels = params.channels.ok_or("undeclared channel count")?.count();
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions::default())
        .map_err(|e| e.to_string())?;
    let mut samples = Vec::new();
    let mut buffer: Option<SampleBuffer<f32>> = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(AudioError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.to_string()),
        };
        if packet.track_id() != id {
            continue;
        }
        let decoded = decoder.decode(&packet).map_err(|e| e.to_string())?;
        if decoded.spec().rate!=rate || decoded.spec().channels.count()!=channels {return Err("decoded signal geometry changed".into());}
        let capacity=decoded.capacity().checked_mul(channels).and_then(|n|n.checked_mul(4)).ok_or("decoded capacity overflow")?;
        if capacity>decoded_byte_limit || capacity>isize::MAX as usize {return Err("decoded conversion capacity exceeds resident budget".into());}
        let samples_needed=decoded.frames().checked_mul(channels).ok_or("decoded sample count overflow")?;
        let buffer = buffer
            .get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, *decoded.spec()));
        if samples_needed>buffer.capacity() {return Err("decoded packet exceeds initial conversion capacity".into());}
        buffer.copy_interleaved_ref(decoded);
        if samples.len().checked_add(buffer.samples().len()).and_then(|n|n.checked_mul(4)).is_none_or(|n|n>decoded_byte_limit) {
            return Err("decoded audio exceeds 512 MiB".into());
        }
        samples.extend_from_slice(buffer.samples());
    }
    if rate == 0 || channels == 0 || samples.is_empty() {
        return Err("invalid audio dimensions".into());
    }
    Ok((samples, channels, rate))
}

/// One sample, or a channel bundle of mono members, as stereo frames: mono
/// is duplicated and channels past the second are left out (reported by the caller).
fn decode_with_initial_playback(parts: &[Vec<u8>],inherit:bool) -> Result<(Decoded, usize, Result<Option<sampler_ir::Playback>,String>), String> {
    if parts.is_empty() || parts.len()>12 {return Err("invalid sample part count".into());}
    let decoded = parts
        .iter()
        .map(|bytes| decode_channels(bytes))
        .collect::<Result<Vec<_>, _>>()?;
    let channels: Vec<(Vec<f32>, usize)> = match decoded.as_slice() {
        [(samples, count, _)] => (0..(*count).min(2))
            .map(|c| (samples.iter().skip(c).step_by(*count).copied().collect(), 1))
            .collect(),
        many => {
            if many
                .iter()
                .any(|(_, count, rate)| *count != 1 || *rate != many[0].2)
            {
                return Err("channel bundle members must be mono at one rate".into());
            }
            many.iter()
                .take(2)
                .map(|(samples, _, _)| (samples.clone(), 1))
                .collect()
        }
    };
    let playback=(|| -> Result<Option<sampler_ir::Playback>,String> { if inherit {
        let frames=channels[0].0.len();let mut metadata=None;
        for (bytes,(samples,count,_)) in parts.iter().zip(&decoded) {
            if *count==0 || samples.len()%*count!=0 || samples.len()/(*count)!=frames {
                return Err("channel bundle has inconsistent initial source geometry".into());
            }
            let found=crate::sample_metadata::parse(bytes,frames)
                .map_err(|e|format!("initial sample playback metadata {e:?}"))?;
            if found.channels.map(usize::from)!=Some(*count) {
                return Err("initial playback channel metadata differs from decoder".into());
            }
            let playback=found.initial_ir_playback().map_err(|e|format!("initial sample playback conversion {e:?}"))?;
            if metadata.is_some_and(|old|old!=playback) {return Err("channel bundle has inconsistent initial playback".into());}
            metadata=Some(playback);
        }
        Ok(metadata)
    } else {Ok(None)} })();
    let total = decoded.iter().map(|(_, count, _)| count).sum();
    let (left, right) = (&channels[0].0, &channels.get(1).unwrap_or(&channels[0]).0);
    let frames = left.iter().zip(right).map(|(&l, &r)| [l, r]).collect();
    Ok((
        Decoded {
            rate: decoded[0].2,
            frames,
        },
        total,playback,
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn mono_wav_becomes_stereo_frames() {
        let mut wav = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x80\xbb\0\0\0\x77\x01\0\x02\0\x10\0data\x04\0\0\0".to_vec();
        wav.extend_from_slice(&[0, 0x40, 0, 0xc0]);
        let (decoded, channels) = super::decode(&[wav]).unwrap();
        assert_eq!((decoded.rate, channels), (48000, 1));
        assert_eq!(decoded.frames, [[0.5, 0.5], [-0.5, -0.5]]);
    }
}


pub(crate) struct InitialDecoded {
    pub source:sampler_core::SourceStorage,
    pub playback:Result<Option<sampler_ir::Playback>,String>,
}
impl InitialDecoded {
    /// Impulses use the existing explicit stereo resource contract. A wide
    /// player's first pack is never an implicit stereo impulse conversion.
    pub fn stereo_audio(&self)->Result<Decoded,String> {
        let pcm=self.source.as_source().stereo().ok_or("wide sample has no declared stereo impulse projection")?;
        let frames=pcm.resident_frames().ok_or("initial impulse is not resident")?;
        Ok(Decoded{rate:pcm.sample_rate(),frames:frames.to_vec()})
    }
}
pub(crate) fn decode(parts:&[Vec<u8>])->Result<(Decoded,usize),String> {
    decode_with_initial_playback(parts,false).map(|(audio,channels,_)|(audio,channels))
}
/// Initial metadata is read only for assets used by an inheriting player.
/// Explicit PlaybackOptions do not depend on unused file-loop data.
pub(crate) fn decode_initial(parts:&[Vec<u8>],inherit:bool)->Result<InitialDecoded,String> {
    if parts.is_empty() || parts.len()>12 {return Err("invalid sample part count".into());}
    let mut decoded=Vec::new();let mut decoded_bytes=0usize;
    for bytes in parts {
        let remaining=MEMORY_LIMIT.checked_sub(decoded_bytes).ok_or("initial source decoded budget exceeded")?;
        let member=decode_channels_with_limit(bytes,remaining)?;
        decoded_bytes=decoded_bytes.checked_add(member.0.len().checked_mul(4).ok_or("initial decoded size overflow")?).ok_or("initial decoded size overflow")?;
        decoded.push(member);
    }
    let rate=decoded[0].2;let frames=decoded[0].0.len().checked_div(decoded[0].1).ok_or("invalid initial channels")?;
    if frames==0 || decoded.iter().any(|(samples,channels,part_rate)|*channels==0 || samples.len()%*channels!=0
        || samples.len()/(*channels)!=frames || *part_rate!=rate) {
        return Err("initial source members have inconsistent geometry".into());
    }
    let channels=if decoded.len()==1 {decoded[0].1} else {
        if decoded.iter().any(|(_,channels,_)|*channels!=1) {return Err("channel bundle members must be mono at one rate".into());}
        decoded.len()
    };
    let playback=(||->Result<Option<sampler_ir::Playback>,String> {
        if !inherit {return Ok(None);}
        let mut metadata=None;
        for (bytes,(_,channels,_)) in parts.iter().zip(&decoded) {
            let found=crate::sample_metadata::parse(bytes,frames).map_err(|e|format!("initial sample playback metadata {e:?}"))?;
            if found.channels.map(usize::from)!=Some(*channels) {return Err("initial playback channel metadata differs from decoder".into());}
            let playback=found.initial_ir_playback().map_err(|e|format!("initial sample playback conversion {e:?}"))?;
            if metadata.is_some_and(|old|old!=playback) {return Err("channel bundle has inconsistent initial playback".into());}
            metadata=Some(playback);
        }
        Ok(metadata)
    })();
    let source=if decoded.len()==1 {
        sampler_kontakt::resident_storage_from_interleaved(rate,channels,&decoded[0].0,MEMORY_LIMIT).map_err(|e|e.to_string())?
    } else {
        let count=frames.checked_mul(channels).ok_or("initial bundle dimensions overflow")?;
        // Charge mono-member input + temporary interleave + physical output.
        let output=frames.checked_mul(channels.div_ceil(2)).and_then(|n|n.checked_mul(8)).ok_or("initial output overflow")?;
        if count.checked_mul(8).and_then(|n|n.checked_add(output)).is_none_or(|n|n>MEMORY_LIMIT) {return Err("initial bundle staging exceeds 512 MiB".into());}
        let mut interleaved=Vec::new();interleaved.try_reserve_exact(count).map_err(|e|e.to_string())?;
        for frame in 0..frames {for (samples,_,_) in &decoded {interleaved.push(samples[frame]);}}
        sampler_kontakt::resident_storage_from_interleaved(rate,channels,&interleaved,MEMORY_LIMIT).map_err(|e|e.to_string())?
    };
    Ok(InitialDecoded{source,playback})
}
