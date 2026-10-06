//! Independent offline composition root; no legacy application or engine dependency.
mod kontakt;
mod render_kontakt;
mod wave;
use sampler_core::{
    Instruction, Limits, Outcome, Pcm, PlanControl, Prepared, Program, Region, Runtime,
};
use sampler_midi::{Applied, Ingress, Packets, TimedPacket, Version};
use std::{
    fs::OpenOptions,
    io::{self, BufWriter, Read, Write},
    path::Path,
};

fn core(error: sampler_core::Error) -> io::Error {
    io::Error::other(format!("native core: {error:?}"))
}

enum Mode {
    Copy,
    Demo,
    Echo,
    Script(sampler_ksp::Script),
    Replace(Pcm),
}

struct Replacement {
    at: usize,
    request: u64,
    control: PlanControl,
}

fn prepare_sample(sample: Pcm, scripted: bool, demo: bool) -> io::Result<Prepared> {
    let rate = sample.sample_rate();
    Prepared::new(
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
            key_low: if scripted { 0 } else { 60 },
            key_high: if scripted { 127 } else { 60 },
            root_key: None,
            velocity_low: 0.0,
            velocity_high: 1.0,
            gain: 1.0,
        }],
        if scripted { 128 } else { 1 },
    )
    .map_err(core)
}

fn render(sample: Pcm, output: &Path, mode: Mode) -> io::Result<()> {
    let demo = !matches!(&mode, Mode::Copy);
    let scripted = matches!(&mode, Mode::Script(_));
    let replacing = matches!(&mode, Mode::Replace(_));
    let rate = sample.sample_rate();
    if replacing && rate < 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "sample rate cannot represent a distinct half-second replacement boundary",
        ));
    }
    let count = if scripted || replacing {
        usize::try_from(u64::from(rate) * 2)
            .map_err(|_| io::Error::other("two-second render exceeds platform frame capacity"))?
    } else {
        sample.frame_count()
    };
    let mut plan = prepare_sample(sample, scripted, demo)?;
    let mut replacement_sample = None;
    let program = match mode {
        Mode::Echo => {
            let play = Instruction::Play {
                transpose: 0,
                velocity: sampler_core::Velocity::Scale(0.75),
                inheritance: sampler_core::Inheritance::Linked,
                duration: sampler_core::Duration::FramesOrGate(rate / 8),
            };
            let program = Program::new(vec![
                Instruction::SetLocal { local: 0, value: 2 },
                play,
                Instruction::Wait(rate / 4),
                Instruction::AddLocal {
                    local: 0,
                    value: -1,
                },
                Instruction::JumpIfZero {
                    local: 0,
                    target: 6,
                },
                Instruction::Jump { target: 1 },
                Instruction::End,
            ])
            .map_err(core)?;
            Some(program)
        }
        Mode::Script(script) => {
            plan = script.bind(plan).map_err(core)?;
            None
        }
        Mode::Replace(sample) => {
            replacement_sample = Some(sample);
            None
        }
        _ => None,
    };
    if let Some(program) = program {
        plan = plan.with_programs(vec![program], Some(0)).map_err(core)?;
    }
    let limits = Limits {
        notes: 32,
        channels: 16,
        performances: 1,
        expressions: 32,
        families: 32,
        decisions: 0,
        voices: 64,
        commands: 64,
        behaviors: 2,
        behavior_fuel: 65536,
        behavior_cells: plan
            .behavior_local_count()
            .checked_mul(2)
            .ok_or_else(|| io::Error::other("callback register budget overflow"))?,
        note_cells: plan
            .note_cell_count()
            .checked_mul(32)
            .ok_or_else(|| io::Error::other("note-state budget overflow"))?,
    };
    let (rt, replacement) = if let Some(sample) = replacement_sample {
        let prepared = prepare_sample(sample, false, true)?;
        let (rt, mut control) = Runtime::with_plan_updates(plan, limits, 2, 1).map_err(core)?;
        let request = control
            .submit(Box::new(prepared))
            .map_err(|e| io::Error::other(format!("replacement plan: {:?}", e.reason)))?;
        (
            rt,
            Some(Replacement {
                at: rate as usize / 2,
                request,
                control,
            }),
        )
    } else {
        (Runtime::new(plan, limits).map_err(core)?, None)
    };
    let mut groups = [None; 16];
    groups[0] = Some(Version::Midi2);
    let mut ingress = Ingress::new(0, groups);
    let events = if replacing {
        [
            (0, [0x4090_3c00, 0xffff_0000]),
            (u64::from(rate) / 2, [0x4090_3c00, 0xffff_0000]),
            (u64::from(rate), [0x4080_3c00, 0]),
            (u64::from(rate) * 3 / 2, [0x4080_3c00, 0]),
        ]
    } else {
        [
            (0, [0x4090_3c00, 0xffff_0000]),
            (u64::from(rate) / 4, [0x40b0_4000, u32::MAX]),
            (u64::from(rate) / 2, [0x4080_3c00, 0x8000_0000]),
            (u64::from(rate), [0x40b0_4000, 0]),
        ]
    };
    let packets = events[..if demo { 4 } else { 1 }]
        .iter()
        .map(|(at, words)| {
            let packet = Packets::new(words)
                .next()
                .ok_or_else(|| io::Error::other("missing demo packet"))?
                .map_err(|e| io::Error::other(format!("UMP framing: {e:?}")))?;
            Ok(TimedPacket {
                offset: *at as usize,
                packet,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    write_render(rt, output, count, &packets, &mut ingress, replacement)
}

fn write_render(
    mut rt: Runtime,
    output: &Path,
    count: usize,
    packets: &[TimedPacket<'_>],
    ingress: &mut Ingress,
    mut replacement: Option<Replacement>,
) -> io::Result<()> {
    let rate = rt.sample_rate();
    let expected_terminals = if replacement.is_some() { 2 } else { 1 };
    let mut terminals = 0;
    let mut next = 0;
    // Validate the output representation before creating any destination.
    wave::header(&mut io::sink(), rate, count)?;
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
        let mut len = remaining.min(buffer.len());
        let begin = count - remaining;
        if let Some(replacement) = &replacement {
            if begin == replacement.at {
                let applied = rt
                    .poll_plan_update()
                    .map_err(|e| io::Error::other(format!("plan adoption: {e:?}")))?;
                if applied != Some(replacement.request) {
                    return Err(io::Error::other(
                        "replacement was not adopted at its boundary",
                    ));
                }
            } else if begin < replacement.at {
                len = len.min(replacement.at - begin);
            }
        }
        let mut batch = [packets[0]; 4];
        let mut event_count = 0;
        while next < packets.len() && packets[next].offset < begin + len {
            batch[event_count] = TimedPacket {
                offset: packets[next].offset - begin,
                ..packets[next]
            };
            event_count += 1;
            next += 1;
        }
        let mut failure = None;
        ingress
            .render(
                &mut rt,
                &mut buffer[..len],
                &batch[..event_count],
                4,
                |_, result| {
                    if !matches!(
                        result,
                        Ok(Applied::Started(_) | Applied::Released { .. } | Applied::Pedal)
                    ) {
                        failure.get_or_insert(result);
                    }
                },
            )
            .map_err(|e| io::Error::other(format!("UMP block: {e:?}")))?;
        if let Some(result) = failure {
            return Err(io::Error::other(format!("UMP input: {result:?}")));
        }
        let mut behavior_failure = None;
        rt.flush_behaviors(|_, _, outcome| {
            if outcome != Outcome::Finished {
                behavior_failure.get_or_insert(outcome);
            }
            true
        });
        if let Some(outcome) = behavior_failure {
            return Err(io::Error::other(format!("native behavior: {outcome:?}")));
        }
        rt.flush_ended(|_| {
            terminals += 1;
            true
        });
        rt.collect_retired_plans();
        if let Some(replacement) = &mut replacement {
            // Outside audio execution: destroy returned assets on the control side.
            drop(replacement.control.retired());
        }
        wave::frames(&mut out, &buffer[..len])?;
        remaining -= len;
    }
    rt.panic();
    rt.flush_behaviors(|_, _, outcome| matches!(outcome, Outcome::Finished | Outcome::Cancelled));
    rt.flush_ended(|_| {
        terminals += 1;
        true
    });
    if terminals != expected_terminals || rt.note_count() != 0 {
        return Err(io::Error::other("incomplete note retirement"));
    }
    if replacement.is_some() && rt.plan_count() != 1 {
        return Err(io::Error::other("incomplete plan retirement"));
    }
    out.flush()?;
    println!(
        "rendered {count} frames at {rate} Hz through sampler-core; {terminals} terminals accepted"
    );
    Ok(())
}

fn demo_sample() -> Pcm {
    let rate = 48000;
    let frames = (0..rate * 2)
        .map(|i| {
            let v = (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.25;
            [v, v]
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    Pcm::new(rate, frames).unwrap()
}

fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.as_slice() {
        [command, input]
            if command == "inspect-kontakt-chunks"
                || command == "inspect-kontakt-nks"
                || command == "inspect-kontakt-nis" =>
        {
            let format = if command == "inspect-kontakt-nks" {
                kontakt::Format::Nks
            } else if command == "inspect-kontakt-nis" {
                kontakt::Format::Nis
            } else {
                kontakt::Format::Chunks
            };
            kontakt::inspect(Path::new(input), format)
        }
        [command, output] if command == "demo" || command == "echo" => render(
            demo_sample(),
            Path::new(output),
            if command == "echo" {
                Mode::Echo
            } else {
                Mode::Demo
            },
        ),
        [command, input, output] if command == "render" => {
            render(wave::read(Path::new(input))?, Path::new(output), Mode::Copy)
        }
        [command, first, second, output] if command == "replace" => render(
            wave::read(Path::new(first))?,
            Path::new(output),
            Mode::Replace(wave::read(Path::new(second))?),
        ),
        [command, source, output] if command == "script" => {
            render_script(demo_sample(), Path::new(source), Path::new(output))
        }
        [command, source, input, output] if command == "script" => render_script(
            wave::read(Path::new(input))?,
            Path::new(source),
            Path::new(output),
        ),
        [command, instrument, output, rest @ ..]
            if command == "render-kontakt" && rest.len() <= 2 =>
        {
            let scripts = !rest.iter().any(|a| a == "--no-scripts");
            let sequence = rest.iter().find(|a| *a != "--no-scripts");
            let sequence = sequence.map_or(Some(render_kontakt::DEFAULT_SEQUENCE), |s| s.to_str());
            let notes = render_kontakt::parse(sequence.unwrap_or(""))
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            render_kontakt::run(Path::new(instrument), Path::new(output), &notes, scripts)
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: sampler-native render-kontakt INPUT.nki OUTPUT.wav [KEY:VEL:START:LENGTH,...] [--no-scripts] | demo OUTPUT.wav | echo OUTPUT.wav | render INPUT.wav OUTPUT.wav | script INPUT.ksp [INPUT.wav] OUTPUT.wav | replace FIRST.wav SECOND.wav OUTPUT.wav | inspect-kontakt-chunks EXPANDED.bin | inspect-kontakt-nks INPUT.nki | inspect-kontakt-nis INPUT.nki",
        )),
    }
}

fn render_script(sample: Pcm, source: &Path, output: &Path) -> io::Result<()> {
    let limits = sampler_ksp::Limits {
        source_bytes: 1 << 20,
        instructions: 65536,
        variables: 128,
        array_cells: 1_000_000,
    };
    let mut text = String::new();
    std::fs::File::open(source)?
        .take(limits.source_bytes as u64 + 1)
        .read_to_string(&mut text)?;
    let program = sampler_ksp::compile(&text, sample.sample_rate(), limits, &[]).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: {e}", source.display()),
        )
    })?;
    render(sample, output, Mode::Script(program))
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
