//! Independent offline composition root; no legacy application or engine dependency.
mod wave;
use sampler_core::{Event, Input, Limits, Pcm, Prepared, Protocol, Region, Runtime};
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
    let input = Input {
        protocol: Protocol::Native,
        port: 0,
        group: 0,
        channel: 0,
        key: 60,
        external_id: Some(1),
    };
    let channel = rt.register_channel(input.channel_address()).map_err(core)?;
    let note = rt.trigger(input, 60, 1.0).map_err(core)?;
    if demo {
        rt.schedule_event(u64::from(rate) / 4, Event::Sustain(channel, true))
            .map_err(core)?;
        rt.schedule_event(u64::from(rate) / 2, Event::KeyUp(note))
            .map_err(core)?;
        rt.schedule_event(u64::from(rate), Event::Sustain(channel, false))
            .map_err(core)?;
    }
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
        let len = remaining.min(buffer.len());
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
