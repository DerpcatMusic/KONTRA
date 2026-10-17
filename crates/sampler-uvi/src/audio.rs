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
        append_initial_packet(&mut samples,&mut buffer,decoded,rate,channels,decoded_byte_limit)?;
    }
    if rate == 0 || channels == 0 || samples.is_empty() {
        return Err("invalid audio dimensions".into());
    }
    Ok((samples, channels, rate))
}

/// Ordinary decode-loop packet conversion, also used by the authored variable
/// capacity witness below. All completed packet samples already live in samples.
/// Replacing the offaudio conversion buffer cannot lose those earlier frames.
fn append_initial_packet(samples:&mut Vec<f32>,buffer:&mut Option<SampleBuffer<f32>>,
    decoded:symphonia::core::audio::AudioBufferRef<'_>,rate:u32,channels:usize,byte_limit:usize)->Result<(),String> {
    if decoded.spec().rate!=rate || decoded.spec().channels.count()!=channels {return Err("decoded signal geometry changed".into());}
    let desired=decoded.capacity().checked_mul(channels).ok_or("decoded capacity overflow")?;
    let needed=decoded.frames().checked_mul(channels).ok_or("decoded sample count overflow")?;
    if needed>desired {return Err("decoded frames exceed packet capacity".into());}
    let replace=buffer.as_ref().is_none_or(|current|needed>current.capacity());
    let conversion=if replace {desired} else {buffer.as_ref().map_or(0,SampleBuffer::capacity)};
    let output_len=samples.len().checked_add(needed).ok_or("decoded output length overflow")?;
    let retained_capacity=samples.capacity().max(output_len);
    let fits=|retained:usize|retained.checked_add(conversion).and_then(|n|n.checked_mul(4))
        .is_some_and(|bytes|bytes<=byte_limit&&bytes<=isize::MAX as usize);
    if !fits(retained_capacity) {return Err("decoded conversion plus retained samples exceed resident budget".into());}
    // Reserve the exact logical append before replacing the conversion buffer.
    // Any allocator overcapacity is checked; these are userspace capacities,
    // not a claim about codec internals, allocator/RSS or transient realloc.
    samples.try_reserve_exact(needed).map_err(|e|e.to_string())?;
    if !fits(samples.capacity()) {return Err("retained decoder capacity exceeds resident budget".into());}
    if replace {
        // The old conversion buffer no longer owns unique PCM. Drop it first
        // rather than hold old+new conversion capacity across this allocation.
        drop(buffer.take());
        *buffer=Some(SampleBuffer::new(decoded.capacity() as u64,*decoded.spec()));
    }
    let conversion=buffer.as_mut().ok_or("missing initial conversion buffer")?;
    conversion.copy_interleaved_ref(decoded);
    if conversion.samples().len()!=needed {return Err("decoded conversion length changed".into());}
    samples.extend_from_slice(conversion.samples());
    Ok(())
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

#[cfg(test)]
mod initial_packet_growth_tests {
    use super::*;
    use symphonia::core::audio::{AsAudioBufferRef,AudioBuffer,Channels,Signal,SignalSpec};
    fn packet(capacity:usize,frames:usize,rate:u32,start:usize)->AudioBuffer<f32> {
        let mut packet=AudioBuffer::new(capacity as u64,SignalSpec::new(rate,Channels::FRONT_LEFT|Channels::FRONT_RIGHT));
        packet.render_reserved(Some(frames));
        for channel in 0..2 {for (frame,to) in packet.chan_mut(channel).iter_mut().enumerate() {*to=(start+frame*2+channel)as f32*0.125;}}
        packet
    }
    #[test]
    fn initial_variable_packet_capacities_append_every_sample_without_resetting_prior_pcm() {
        let mut samples=Vec::new();let mut buffer=None;let mut expected=Vec::new();
        for (capacity,frames,start) in [(8,4,0),(40,32,8),(12,3,72)] {
            let packet=packet(capacity,frames,48000,start);
            for frame in 0..frames {for channel in 0..2 {expected.push((start+frame*2+channel)as f32*0.125);}}
            append_initial_packet(&mut samples,&mut buffer,packet.as_audio_buffer_ref(),48000,2,65536).unwrap();
            assert_eq!(samples,expected);
        }
        assert_eq!(samples.len(),78);assert_eq!(buffer.unwrap().capacity(),80);
    }
    #[test]
    fn initial_packet_growth_refuses_before_replacing_buffer_when_joint_budget_is_short() {
        let mut samples=Vec::new();let mut buffer=None;
        let first=packet(8,4,48000,0);append_initial_packet(&mut samples,&mut buffer,first.as_audio_buffer_ref(),48000,2,128).unwrap();
        let prior=samples.clone();let capacity=buffer.as_ref().unwrap().capacity();let second=packet(40,32,48000,8);
        // 72 retained samples +80 conversion samples =608 bytes.
        assert!(append_initial_packet(&mut samples,&mut buffer,second.as_audio_buffer_ref(),48000,2,607).is_err());
        assert_eq!(samples,prior);assert_eq!(buffer.as_ref().unwrap().capacity(),capacity);
        append_initial_packet(&mut samples,&mut buffer,second.as_audio_buffer_ref(),48000,2,608).unwrap();
        assert_eq!(samples.len(),72);assert_eq!(buffer.unwrap().capacity(),80);
    }
    #[test]
    fn initial_packet_rate_change_refuses_without_losing_completed_samples() {
        let mut samples=Vec::new();let mut buffer=None;
        let first=packet(8,4,48000,0);append_initial_packet(&mut samples,&mut buffer,first.as_audio_buffer_ref(),48000,2,4096).unwrap();
        let prior=samples.clone();let capacity=buffer.as_ref().unwrap().capacity();let wrong=packet(40,32,44100,8);
        assert!(append_initial_packet(&mut samples,&mut buffer,wrong.as_audio_buffer_ref(),48000,2,4096).is_err());
        assert_eq!(samples,prior);assert_eq!(buffer.as_ref().unwrap().capacity(),capacity);
    }
}
