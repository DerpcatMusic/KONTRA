//! `dspbench`: runs every pair over rates x block sizes and writes the verdict report.
//!
//! Usage: `dspbench [--out DIR] [--bands FILE.json] [--no-perf]`. The CPU instruction count uses
//! `perf stat -e instructions:u` re-executing this binary as `--cpu-child` (the perf-less numbers
//! are ns/frame only). `--cpu-child` is internal.

use sampler_dspbench::{band::Band, default_params, impls, measure, pairs, report, signal, PAIRS};
use std::process::Command;

const RATES: [f64; 3] = [44_100.0, 48_000.0, 96_000.0];
const BLOCKS: [usize; 3] = [32, 64, 512];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--cpu-child") {
        return cpu_child(&args[1..]);
    }
    let mut out_dir = "docs/research/dspbench".to_string();
    let mut band = Band::default();
    let mut perf = true;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--out" => out_dir = it.next().expect("--out DIR").clone(),
            "--bands" => {
                let text = std::fs::read_to_string(it.next().expect("--bands FILE")).expect("read bands");
                band = serde_json::from_str(&text).expect("bands json");
            }
            "--no-perf" => perf = false,
            other => panic!("unknown argument {other}"),
        }
    }

    let mut reports = Vec::new();
    // The resampler is a rate converter, not a block: one run at 48 -> 44.1 kHz, no instruction count.
    if let Some(cand) = sampler_dspbench::pairs::resampler::cand_src() {
        let r = sampler_dspbench::pairs::resampler::run(&band, sampler_dspbench::pairs::resampler::ours_src(), cand);
        eprintln!("resampler: done");
        reports.push(r);
    }
    for pair in PAIRS.into_iter().filter(|p| *p != "resampler") {
        let (ours, cand) = impls(pair);
        for rate in RATES {
            for block in BLOCKS {
                // Fidelity is fitted once, at the oracle's own 48 kHz / 64-frame point.
                let fit = block == 64;
                let mut r = match pair {
                    "filter-lp2" => pairs::filter::run(&band, ours, cand, rate, block, fit),
                    "limiter" => pairs::limiter::run(&band, ours, cand.expect("limiter candidate"), rate, block),
                    "eq-bell" => pairs::eq::run(&band, ours, cand, rate, block, fit),
                    "oversampler" => pairs::oversampler::run(&band, ours, cand, rate, block),
                    "compressor" => pairs::compressor::run(&band, ours, cand, rate, block),
                    "svf-ladder" => pairs::svf::run(&band, ours, cand, rate, block, fit),
                    _ => unreachable!(),
                };
                if perf {
                    for (i, side) in r.sides.iter_mut().enumerate() {
                        side.quality.instr_per_frame = instructions_per_frame(pair, i, rate, block);
                    }
                }
                eprintln!("{pair} {rate} Hz block {block}: done");
                reports.push(r);
            }
        }
    }

    std::fs::create_dir_all(&out_dir).expect("out dir");
    std::fs::write(format!("{out_dir}/results.json"), serde_json::to_string_pretty(&reports).expect("json")).expect("write json");
    std::fs::write(format!("{out_dir}/grid.csv"), report::csv(&reports)).expect("write csv");
    let machine = format!("{} {} cores, {}", std::env::consts::OS, std::thread::available_parallelism().map_or(1, |n| n.get()), std::env::consts::ARCH);
    std::fs::write(format!("{out_dir}/report.md"), report::markdown(&reports, &band, &machine)).expect("write md");
    eprintln!("wrote {out_dir}/report.md, results.json, grid.csv");
}

/// Instructions per frame: `perf stat` on a child that processes one second of noise, minus the
/// same child without processing (so the noise generator's own cost cancels).
fn instructions_per_frame(pair: &str, which: usize, rate: f64, block: usize) -> Option<f64> {
    let frames = rate as usize;
    let run = |mode: &str| -> Option<f64> {
        let exe = std::env::current_exe().ok()?;
        let out = Command::new("perf")
            .args(["stat", "-x,", "-e", "instructions:u", "--"])
            .arg(exe)
            .args(["--cpu-child", pair, &which.to_string(), &rate.to_string(), &block.to_string(), mode, &frames.to_string()])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stderr);
        text.lines().find(|l| l.contains("instructions"))?.split(',').next()?.trim().parse::<f64>().ok()
    };
    let (with_proc, without) = (run("proc")?, run("gen")?);
    Some((with_proc - without) / frames as f64)
}

fn cpu_child(a: &[String]) {
    let [pair, which, rate, block, mode, frames] = a else { panic!("cpu-child args") };
    let (ours, cand) = impls(pair);
    let imp = if which == "0" { ours } else { cand.expect("candidate") };
    let rate: f64 = rate.parse().expect("rate");
    let block: usize = block.parse().expect("block");
    let frames: usize = frames.parse().expect("frames");
    let label = imp.label;
    let x = signal::white(frames, 0.5, 7);
    if mode == "proc" {
        let params = default_params(pair, label);
        let mut b = (imp.make)(rate, &params);
        let mut out = vec![0.0f32; block];
        for chunk in x.chunks(block) {
            b.process(chunk, &mut out[..chunk.len()]);
        }
        std::hint::black_box(&out);
    } else {
        std::hint::black_box(&x);
    }
    let _ = measure::FFT_LEN;
}
