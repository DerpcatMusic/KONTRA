//! UVI algorithmic reverbs: attribute names (plain and element-prefixed) and
//! the mix scale (normalized `0..1`, as observed in factory programs).

use sampler_ir::Processor;
use sampler_ir::fx::space::{Config, Topology};
use std::path::Path;

fn spaces(inserts: &str) -> Vec<Config> {
    let xml = format!("<UVI4><Program><Inserts>{inserts}</Inserts></Program></UVI4>");
    let uvi = sampler_uvi::translate(&xml, Path::new(".")).unwrap();
    uvi.instrument
        .chains
        .iter()
        .flat_map(|c| c.pre_amplitude.iter().chain(&c.post_amplitude))
        .filter_map(|p| match p {
            Processor::Space(config) => Some(*config),
            _ => None,
        })
        .collect()
}

fn one(inserts: &str) -> Config {
    let found = spaces(inserts);
    assert_eq!(found.len(), 1, "{inserts}");
    found[0]
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() < 1e-9, "{a} != {b}");
}

#[test]
fn plain_and_prefixed_names_give_the_same_network() {
    let plain = one("<PlainReverb DecayTime='3' HighDamp='0.5' Mix='0.25'/>");
    let saved = one("<PlainReverb ReverbDecayTime='3' ReverbHighDamp='0.5' WetMix='0.25'/>");
    assert_eq!(plain, saved);
    let net = plain.network.unwrap();
    assert_eq!(net.topology, Topology::CombAllpass);
    close(net.decay_seconds, 3.0);
    close(plain.dry, 0.75);
    close(plain.wet, 0.25);
}

#[test]
fn gate_reverb_reads_gate_and_predelay() {
    let net = one("<GateReverb ReverbGateTime='0.2' ReverbPreDelay='0.1' WetMix='0.3'/>").network.unwrap();
    close(net.gate_seconds.unwrap(), 0.2);
    close(net.predelay_seconds, 0.1);
}

#[test]
fn delayed_reverb_reads_feedback() {
    let net = one("<DelayedReverb Feedback='0.4' PreDelay='0.3'/>").network.unwrap();
    close(net.echo_feedback, 0.4);
    close(net.predelay_seconds, 0.3);
}

#[test]
fn filtered_reverb_is_a_comb_allpass_reverb() {
    let c = one("<FilteredReverb ReverbDecayTime='1.5' WetMix='0.5'/>");
    close(c.network.unwrap().decay_seconds, 1.5);
    close(c.wet, 0.5);
}

#[test]
fn spark_verb_runs_its_native_engine() {
    let c = one("<SparkVerb PreDelay='20' DecayTime='4' Width='0.5' Mix='0.25'/>");
    let net = c.network.unwrap();
    assert_eq!(net.topology, Topology::SparkVerb);
    close(net.predelay_seconds, 0.02);
    // RT60 is 0.1 * RoomSize (20 m default) * DecayTime.
    close(net.decay_seconds, 8.0);
    let spark = net.spark.unwrap();
    assert_eq!((spark.quality, spark.mode, spark.diffuser), (3, 1, None));
    // Equal-power mix by default (MixMode 1).
    close(c.dry, 0.75f64.sqrt());
    close(c.wet, 0.5);
}

#[test]
fn bloom_room_size_scales_the_network() {
    let net = one("<Bloom RoomSize='10' DecayTime='2'/>").network.unwrap();
    close(net.size, 0.5);
    close(net.decay_seconds, 2.0);
}

#[test]
fn diffusion_is_a_diffuser_with_full_wet_default() {
    let c = one("<Diffusion Diffusion='1' DiffusionStart='10'/>");
    assert_eq!(c.network.unwrap().topology, Topology::Diffuser);
    close(c.network.unwrap().diffusion, 0.75);
    close(c.wet, 1.0);
}
