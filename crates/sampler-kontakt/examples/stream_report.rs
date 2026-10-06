//! Memory/CPU report for a real instrument: `stream_report NKI`.
use std::path::Path;

fn main() {
    let path = std::env::args().nth(1).expect("usage: stream_report NKI");
    let mut kontakt = sampler_kontakt::read(Path::new(&path)).unwrap();
    let (mut frames, mut archived) = (0u64, 0usize);
    let mut sizes = Vec::new();
    for location in &kontakt.locations {
        let n = kontakt.samples.frames(location).unwrap();
        frames += n;
        sizes.push(n);
        archived += usize::from(!location.is_file());
    }
    sizes.sort_unstable();
    println!(
        "{path}: {} zones, {} assets ({archived} archive members), {frames} frames, \
         full f32 stereo {:.1} MB, median asset {} frames, max {}",
        kontakt.instrument.zones.len(),
        kontakt.locations.len(),
        frames as f64 * 8. / 1e6,
        sizes.get(sizes.len() / 2).copied().unwrap_or(0),
        sizes.last().copied().unwrap_or(0),
    );
}
