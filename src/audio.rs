use anyhow::{Context, Result, ensure};
use std::{fs::File, path::{Path,PathBuf}, collections::HashMap, io::Cursor};
use symphonia::core::{audio::SampleBuffer, codecs::DecoderOptions, formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint};

pub struct Sample { pub rate: u32, pub frames: Vec<[f32; 2]> }

#[derive(Default)]
pub struct Decoder { archives: HashMap<PathBuf,(ni_file::nkr::Archive,Option<ni_file::nis::LibraryKey>)> }
pub fn decode(path: &Path, max_frames: usize) -> Result<Sample> { Decoder::default().decode(path,max_frames) }
impl Decoder {
pub fn decode(&mut self, path: &Path, max_frames: usize) -> Result<Sample> {
    let file: Box<dyn symphonia::core::io::MediaSource> = if let Some((archive,member))=crate::import::archive_member(path) {
        if !self.archives.contains_key(&archive) { self.archives.insert(archive.clone(), (ni_file::nkr::Archive::read(File::open(&archive)?)?,crate::import::library_key(&archive)?)); }
        let (index,key)=&self.archives[&archive];
        let bytes=index.read_entry_with_key(File::open(&archive)?,&member,key.as_ref())?;
        Box::new(Cursor::new(bytes))
    } else { Box::new(File::open(path).with_context(|| format!("Opening sample {}",path.display()))?) };
    let result = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ncw")) {
        let mut r = ncw::NcwReader::read(file)?;
        ensure!((1..=2).contains(&r.header.channels), "Only mono/stereo samples are supported");
        ensure!(r.header.num_samples as usize <= max_frames, "Sample exceeds remaining memory budget");
        let channels = r.header.channels as usize;
        let scale = 2f32.powi(r.header.bits_per_sample as i32 - 1);
        let float = r.sample_format == ncw::SampleFormat::Float;
        let rate = r.header.sample_rate;
        let pcm = r.decode_samples()?;
        let sample = |s: i32| if float { f32::from_bits(s as u32) } else { s as f32 / scale };
        Sample { rate, frames: pcm.chunks_exact(channels).map(|s| [sample(s[0]), sample(s[channels-1])]).collect() }
    } else {
        let mut hint = Hint::new();
        if let Some(e)=path.extension().and_then(|s|s.to_str()) { hint.with_extension(e); }
        let mss = MediaSourceStream::new(file, Default::default());
        let mut format = symphonia::default::get_probe().format(&hint,mss,&FormatOptions::default(),&MetadataOptions::default())?.format;
        let track = format.default_track().context("No audio track")?;
        if let Some(n)=track.codec_params.n_frames { ensure!(n <= max_frames as u64, "Sample exceeds remaining memory budget"); }
        let track_id = track.id;
        let mut decoder = symphonia::default::get_codecs().make(&track.codec_params,&DecoderOptions::default())?;
        let mut frames = Vec::new();
        let mut rate = track.codec_params.sample_rate.unwrap_or(0);
        loop {
            let packet = match format.next_packet() {
                Ok(p)=>p,
                Err(symphonia::core::errors::Error::IoError(e)) if e.kind()==std::io::ErrorKind::UnexpectedEof => break,
                Err(e)=>return Err(e.into()),
            };
            if packet.track_id()!=track_id { continue; }
            let decoded=decoder.decode(&packet)?;
            let spec=*decoded.spec(); let channels=spec.channels.count();
            ensure!((1..=2).contains(&channels), "Only mono/stereo samples are supported");
            ensure!(rate == 0 || rate == spec.rate,"Sample rate changed mid-file"); rate=spec.rate;
            ensure!(frames.len()+decoded.frames() <= max_frames,"Sample exceeds remaining memory budget");
            let mut pcm=SampleBuffer::<f32>::new(decoded.capacity() as u64,spec);pcm.copy_interleaved_ref(decoded);
            frames.extend(pcm.samples().chunks_exact(channels).map(|s|[s[0],s[channels-1]]));
        }
        Sample {rate,frames}
    };
    ensure!(result.rate > 0 && !result.frames.is_empty(),"Empty sample or invalid rate");
    ensure!(result.frames.iter().flatten().all(|s|s.is_finite()),"Sample contains non-finite audio");
    Ok(result)
}

}
