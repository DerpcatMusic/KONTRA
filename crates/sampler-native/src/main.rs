//! Independent offline composition root; no legacy application or engine dependency.
mod wave;
use sampler_core::{Limits, Pcm, Prepared, Region, Runtime};
use sampler_midi::{Ingress, Packets, Version};
use std::{
    fs::OpenOptions,
    io::{self, BufWriter, Write},
    path::Path,
};

fn core(error: sampler_core::Error) -> io::Error {
    io::Error::other(format!("native core: {error:?}"))
}

fn render(sample: Pcm, output: &Path, demo: bool) -> io::Result<()> {
    let rate = sample.rate;
    let count = sample.frames.len();
    let plan = Prepared::new(
        rate,
        vec![sample],
        vec![Region {
            playback: sampler_core::Playback::default(),
            envelope: if demo {
                sampler_core::Envelope::new(rate / 200, 0, rate / 10, 0.8, rate / 20)
                    .map_err(core)?
            } else {
                sampler_core::Envelope::default()
            },
            sample: 0,
            key_low: 60,
            key_high: 60,
            velocity_low: 0.0,
            velocity_high: 1.0,
            gain: 1.0,
        }],
        1,
    )
    .map_err(core)?;
    let mut rt = Runtime::new(
        plan,
        Limits {
            notes: 32,
            channels: 16,
            expressions: 32,
            families: 32,
            voices: 64,
            commands: 64,
        },
    )
    .map_err(core)?;
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi2);
    let ingress = Ingress::new(0, groups);
    let events = [
        (0, [0x4090_3c00, 0xffff_0000]),
        (u64::from(rate) / 4, [0x40b0_4000, u32::MAX]),
        (u64::from(rate) / 2, [0x4080_3c00, 0x8000_0000]),
        (u64::from(rate), [0x40b0_4000, 0]),
    ];
    let mut events = events[..if demo { 4 } else { 1 }].iter().peekable();
    // Refuse overwrites, including an input path reused as output.
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut out = BufWriter::new(file);
    wave::header(&mut out, rate, count)?;
    let mut buffer = [[0.0; 2]; 256];
    let mut remaining = count;
    while remaining > 0 {
        while let Some((at, words)) = events.peek()
            && *at == rt.now()
        {
            for packet in Packets::new(words) {
                let packet = packet.map_err(|e| io::Error::other(format!("UMP framing: {e:?}")))?;
                ingress
                    .apply(&mut rt, packet)
                    .map_err(|e| io::Error::other(format!("UMP input: {e:?}")))?;
            }
            events.next();
        }
        let boundary = events
            .peek()
            .map_or(remaining, |(at, _)| (*at - rt.now()) as usize);
        let len = remaining.min(buffer.len()).min(boundary);
        rt.render(&mut buffer[..len]).map_err(core)?;
        wave::frames(&mut out, &buffer[..len])?;
        remaining -= len;
    }
    rt.panic();
    let mut terminals = 0;
    rt.flush_ended(|_| {
        terminals += 1;
        true
    });
    if terminals != 1 || rt.note_count() != 0 {
        return Err(io::Error::other("incomplete note retirement"));
    }
    out.flush()?;
    println!("rendered {count} frames at {rate} Hz through sampler-core; one terminal accepted");
    Ok(())
}

fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, output] if command == "demo" => {
            let rate = 48000;
            let frames = (0..rate * 2)
                .map(|i| {
                    let v = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.25;
                    [v, v]
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            render(Pcm { rate, frames }, Path::new(output), true)
        }
        [command, input, output] if command == "render" => {
            render(wave::read(Path::new(input))?, Path::new(output), false)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: sampler-native demo OUTPUT.wav | render INPUT.wav OUTPUT.wav",
        )),
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
