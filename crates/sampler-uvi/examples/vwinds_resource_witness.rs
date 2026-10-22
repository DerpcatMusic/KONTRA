//! PRIVATE candidate: root registers/builds/runs separately after inspection.
//! Ordinary Bank/translation APIs only; header metadata, no authored execution
//! execution. Optional admitted original-channel PCM is reduced to numbers in memory.
#[path = "../src/audio.rs"]
mod audio;
#[path = "../src/sample_metadata.rs"]
mod sample_metadata;
#[path = "../src/placement.rs"]
mod placement;

use sampler_core::{LoopMode, LoopShape};
use sampler_uvi::{Bank, translate_path};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, io::Cursor, path::Path};
use symphonia::core::{
    formats::FormatOptions,
    io::MediaSourceStream,
    meta::{Limit, MetadataOptions},
    probe::Hint,
};

const BANK: &str = "/mnt/MAIN_STORAGE/Libraries/UVI/VWinds - Clarinets/VWinds-AClarinet.ufs";
const MEMBER: &str = "Presets/Clarinet A.uvip";
const BANK_SHA: &str = "e6e16c03efc9e3a39484d6326b9ba5634314bc896d6523c83e661b1b279a884a";
const WORKER_SHA: &str = "6609fc91cf267888d6b012843b60767703dcebdf9ed741a95b30bdd4dab812ec";
const WORKER_BYTES: usize = 60;
const CANDIDATE_ASSETS: [usize; 4] = [50, 47, 52, 163];
const MAX_BYTES: u64 = 128 << 20;
const MAX_DECODED_BYTES: u64 = 64 << 20;
const MAX_CHANNELS: usize = 12;
const MAX_CORRELATION_PAIRS: u64 = 128_000_000;
const CORRELATION_FRAMES: usize = 2048;
const MIN_LAG: usize = 16;
const MAX_LAG: usize = 800;
const MAX_PEAKS: usize = 8;
const PAYLOAD_BYTES: usize = 816693;
const PAYLOAD_SHA: &str = "06cc62e2308af040f85414f976a346326976bc402d9aecc0468f7ca0addf3bf5";
const SOURCE_RATE: u32 = 44100;
const SOURCE_FRAMES: u64 = 196510;
const SOURCE_CHANNELS: usize = 6;

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone, Copy)]
enum Mode { Join, Metadata }

struct Request {
    mode: Mode,
    selected: BTreeSet<usize>,
    runtime: bool,
    byte_limit: u64,
    numeric_windows: bool,
}

fn request() -> Result<Request, &'static str> {
    let mut mode = None;
    let mut attested = false;
    let mut selected = BTreeSet::new();
    let mut runtime = false;
    let mut byte_limit = MAX_BYTES;
    let mut numeric_windows = false;
    for arg in std::env::args().skip(1) {
        if arg == "--join-only" {
            if mode.is_some() { return Err("duplicate_mode"); }
            mode = Some(Mode::Join);
        } else if arg == "--metadata-only" {
            if mode.is_some() { return Err("duplicate_mode"); }
            mode = Some(Mode::Metadata);
        } else if arg == "--numeric-windows" {
            if numeric_windows { return Err("duplicate_numeric_mode"); }
            numeric_windows = true;
        } else if let Some(value) = arg.strip_prefix("--root-bank-sha=") {
            if attested || value != BANK_SHA { return Err("bank_identity_attestation"); }
            // Root independently hashes the actual file before/after the run.
            // This argument is an attestation, not an internal file SHA check.
            attested = true;
        } else if let Some(value) = arg.strip_prefix("--max-bytes=") {
            byte_limit = value.parse().map_err(|_| "invalid_byte_budget")?;
            if byte_limit == 0 || byte_limit > MAX_BYTES { return Err("invalid_byte_budget"); }
        } else if let Some(value) = arg.strip_prefix("--select=") {
            if value == "runtime" {
                if runtime { return Err("duplicate_selection"); }
                runtime = true;
            } else {
                let asset = value.parse().map_err(|_| "invalid_asset_index")?;
                if !CANDIDATE_ASSETS.contains(&asset) || !selected.insert(asset) {
                    return Err("unapproved_or_duplicate_asset");
                }
            }
        } else {
            return Err("unknown_argument");
        }
    }
    if !attested { return Err("bank_identity_attestation"); }
    let mode = mode.ok_or("mode_required")?;
    if numeric_windows && !matches!(mode, Mode::Metadata) {
        return Err("numeric_windows_require_metadata_mode");
    }
    Ok(Request { mode, selected, runtime, byte_limit, numeric_windows })
}

fn error_code(error: sample_metadata::MetadataError) -> u8 {
    use sample_metadata::MetadataError as E;
    match error {
        E::Format => 1, E::Truncated => 2, E::Limit => 3, E::Invalid => 4,
        E::UnsupportedVendorSamplerData => 5, E::UnsupportedFractionalLoop => 6,
        E::UnsupportedConflictingLoops => 7,
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct NumericBudget { decoded_bytes: u64, correlation_pairs: u64 }

#[derive(Clone, Copy, Debug)]
struct WindowFrames { start: usize, end: usize }

#[derive(Debug)]
struct NumericPlan {
    declared_samples: u64,
    decoded_bytes: u64,
    correlation_pairs: u64,
    windows: Vec<WindowFrames>,
}

fn numeric_preflight(frames: u64, channels: usize, rate: u32,
    budget: &mut NumericBudget) -> Result<NumericPlan, &'static str>
{
    if frames == 0 || channels == 0 || channels > MAX_CHANNELS || rate == 0 {
        return Err("invalid_numeric_geometry");
    }
    let samples = frames.checked_mul(channels as u64).ok_or("decoded_size_overflow")?;
    let bytes = samples.checked_mul(std::mem::size_of::<f32>() as u64)
        .ok_or("decoded_size_overflow")?;
    let total_bytes = budget.decoded_bytes.checked_add(bytes).ok_or("decoded_size_overflow")?;
    if total_bytes > MAX_DECODED_BYTES { return Err("decoded_logical_budget"); }
    let mut windows = Vec::with_capacity(4);
    let mut pairs = 0u64;
    // Exact source-clock rational endpoints; floor each endpoint to a frame.
    for (a,b,c,d) in [(0u64,8u64,1u64,8u64),(1,4,1,2),(3,4,1,1),(7,4,2,1)] {
        let start = a.checked_mul(u64::from(rate)).ok_or("window_size_overflow")? / b;
        let end = c.checked_mul(u64::from(rate)).ok_or("window_size_overflow")? / d;
        if start >= end || end > frames { return Err("source_window_unavailable"); }
        let start = usize::try_from(start).map_err(|_|"window_size_overflow")?;
        let end = usize::try_from(end).map_err(|_|"window_size_overflow")?;
        let n = (end-start).min(CORRELATION_FRAMES);
        if MAX_LAG >= n/2 { return Err("correlation_lag_not_below_half_slice"); }
        let mut channel_pairs = 0u64;
        for lag in MIN_LAG..=MAX_LAG {
            channel_pairs = channel_pairs.checked_add((n-lag) as u64)
                .ok_or("correlation_work_overflow")?;
        }
        pairs = pairs.checked_add(channel_pairs.checked_mul(channels as u64)
            .ok_or("correlation_work_overflow")?).ok_or("correlation_work_overflow")?;
        windows.push(WindowFrames {start,end});
    }
    let total_pairs = budget.correlation_pairs.checked_add(pairs)
        .ok_or("correlation_work_overflow")?;
    if total_pairs > MAX_CORRELATION_PAIRS { return Err("correlation_work_budget"); }
    // A refused preflight has no partial admission or decoder invocation.
    *budget = NumericBudget { decoded_bytes:total_bytes, correlation_pairs:total_pairs };
    Ok(NumericPlan {declared_samples:samples,decoded_bytes:bytes,correlation_pairs:pairs,windows})
}

fn dbfs(value: f64) -> Option<f64> {
    (value > 0. && value.is_finite()).then(||20.*value.log10())
}

fn channel_metrics(samples: &[f32], channels: usize, channel: usize, rate: u32,
    window: WindowFrames) -> Result<Value, &'static str>
{
    if channels == 0 || channel >= channels || samples.len()%channels != 0
        || window.start >= window.end || window.end > samples.len()/channels || rate == 0
    { return Err("invalid_analysis_geometry"); }
    let count = window.end-window.start;
    let mut sum = 0.;
    let mut squares = 0.;
    let mut peak = 0f64;
    for frame in window.start..window.end {
        let value = f64::from(samples[frame*channels+channel]);
        if !value.is_finite() { return Err("nonfinite_original_channel"); }
        sum += value; squares += value*value; peak = peak.max(value.abs());
    }
    let dc = sum/count as f64;
    let mut ac_squares = 0.;
    let mut ac_peak = 0f64;
    for frame in window.start..window.end {
        let value = f64::from(samples[frame*channels+channel])-dc;
        ac_squares += value*value; ac_peak = ac_peak.max(value.abs());
    }
    let rms = (squares/count as f64).sqrt();
    let ac_rms = (ac_squares/count as f64).sqrt();
    let n = count.min(CORRELATION_FRAMES);
    if MAX_LAG >= n/2 { return Err("correlation_lag_not_below_half_slice"); }
    let corr_mean = (window.start..window.start+n)
        .map(|frame|f64::from(samples[frame*channels+channel])).sum::<f64>()/n as f64;
    let centered: Vec<f64> = (window.start..window.start+n)
        .map(|frame|f64::from(samples[frame*channels+channel])-corr_mean).collect();
    let energy = centered.iter().map(|x|x*x).sum::<f64>();
    let mut scores = vec![f64::NAN; MAX_LAG+1];
    let mut pairs = 0u64;
    if energy > 0. {
        for lag in MIN_LAG..=MAX_LAG {
            let (mut dot,mut left,mut right) = (0.,0.,0.);
            for i in 0..n-lag {
                let (a,b) = (centered[i],centered[i+lag]);
                dot += a*b; left += a*a; right += b*b;
            }
            pairs += (n-lag) as u64;
            if left > 0. && right > 0. {
                scores[lag] = (dot/(left*right).sqrt()).clamp(-1.,1.);
            }
        }
    }
    let mut maxima: Vec<(usize,f64)> = (MIN_LAG..=MAX_LAG).filter_map(|lag| {
        let score = scores[lag];
        if !score.is_finite() || score <= 0. { return None; }
        let left_ok = lag == MIN_LAG || scores[lag-1].is_finite() && score > scores[lag-1];
        let right_ok = lag == MAX_LAG || scores[lag+1].is_finite() && score >= scores[lag+1];
        (left_ok && right_ok).then_some((lag,score))
    }).collect();
    maxima.sort_by(|(la,a),(lb,b)|b.total_cmp(a).then(la.cmp(lb)));
    maxima.truncate(MAX_PEAKS);
    let peaks: Vec<Value> = maxima.into_iter().map(|(lag,score)| {
        let boundary = lag == MIN_LAG || lag == MAX_LAG;
        let delta = if !boundary && scores[lag-1].is_finite() && scores[lag+1].is_finite() {
            let denominator = scores[lag-1]-2.*score+scores[lag+1];
            if denominator < 0. {
                (0.5*(scores[lag-1]-scores[lag+1])/denominator).clamp(-0.5,0.5)
            } else { 0. }
        } else { 0. };
        let refined = lag as f64+delta;
        json!({"lag_frames":lag,"refined_lag_frames":refined,
            "source_hz":f64::from(rate)/refined,"normalized_correlation":score,
            "search_boundary":boundary})
    }).collect();
    Ok(json!({"channel":channel,"frames":count,"finite":true,
        "dc":dc,"rms":rms,"rms_dbfs":dbfs(rms),"ac_rms":ac_rms,
        "ac_rms_dbfs":dbfs(ac_rms),"peak":peak,"ac_peak":ac_peak,
        "periodicity":{"status":if energy>0.{"positive_candidates_only"}else{"silent_or_dc"},
            "slice_start_frame":window.start,"slice_frames":n,
            "slice_dc":corr_mean,"min_lag":MIN_LAG,"max_lag":MAX_LAG,
            "lag_strictly_below_half_slice":MAX_LAG<n/2,
            "sample_pairs_evaluated":pairs,"peaks":peaks}}))
}

fn analyze_windows(samples: &[f32], channels: usize, rate: u32, plan: &NumericPlan)
    -> Result<Vec<Value>, &'static str>
{
    if samples.len() as u64 != plan.declared_samples { return Err("analysis_size_mismatch"); }
    plan.windows.iter().enumerate().map(|(index,window)| {
        let channels = (0..channels).map(|channel|
            channel_metrics(samples,channels,channel,rate,*window)).collect::<Result<Vec<_>,_>>()?;
        Ok(json!({"index":index,"source_rate":rate,"source_start_frame":window.start,
            "source_end_frame_exclusive":window.end,
            "source_start_seconds":window.start as f64/f64::from(rate),
            "source_end_seconds":window.end as f64/f64::from(rate),"channels":channels}))
    }).collect()
}

fn inspect_part(bytes: Vec<u8>, ordinal: usize, mut budget: Option<&mut NumericBudget>) -> Value {
    // A byte-for-byte identity admitted by root, checked BEFORE even retaining
    // the extra encoded copy for the optional ordinary full decoder.
    if budget.is_some() && (bytes.len() != PAYLOAD_BYTES || hash(&bytes) != PAYLOAD_SHA) {
        return json!({"part":ordinal,"status":"numeric_payload_identity_refused",
            "pcm_decoded":false});
    }
    let encoded_for_decode = budget.as_ref().map(|_| bytes.clone());
    let mut result = json!({
        "part": ordinal, "encoded_bytes": bytes.len(), "encoded_sha256": hash(&bytes),
        "pcm_decoded": false, "raw_file_loop_type": null,
        "raw_file_loop_type_status": "not_exposed_by_existing_normalized_parser",
        "header_source": "existing_Symphonia_probe_codec_params",
        "metadata_geometry_override": false,
    });
    // The owned Vec moves into the existing public format probe. No clone,
    // custom reader, decoder construction, next_packet or decode is introduced.
    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let limits = MetadataOptions {
        limit_metadata_bytes: Limit::Maximum(2 << 20),
        limit_visual_bytes: Limit::Maximum(2 << 20),
    };
    let probed = symphonia::default::get_probe()
        .format(&Hint::new(), stream, &FormatOptions::default(), &limits);
    let format = match probed {
        Ok(probed) => probed.format,
        Err(_) => { result["status"] = json!("header_probe_refused"); return result; }
    };
    let params = match format.default_track() {
        Some(track) => track.codec_params.clone(),
        None => { result["status"] = json!("no_default_track"); return result; }
    };
    let rate = params.sample_rate;
    let channels = params.channels.map(|channels| channels.count());
    let frames = params.n_frames;
    result["rate"] = json!(rate);
    result["original_encoded_channels"] = json!(channels);
    result["declared_frames"] = json!(frames);
    result["declared_bits_per_sample"] = json!(params.bits_per_sample);
    let Some(frames) = frames.and_then(|frames| usize::try_from(frames).ok()) else {
        result["status"] = json!("declared_frame_count_unavailable_no_scan");
        return result;
    };
    if frames == 0 || rate.is_none_or(|rate| rate == 0)
        || channels.is_none_or(|channels| channels == 0)
    {
        result["status"] = json!("invalid_declared_geometry");
        return result;
    }
    // Reuse production chunk/loop/geometry parsing on the SAME original byte
    // view after the probe. Do not repair headers or invent sampler defaults.
    let mut reader = format.into_inner();
    match sample_metadata::parse_reader(&mut reader, frames, false) {
        Err(error) => {
            result["status"] = json!("metadata_parser_refused");
            result["metadata_error_code"] = json!(error_code(error));
        }
        Ok(metadata) => {
            result["status"] = json!("metadata_observed");
            result["format_code"] = json!(match metadata.format {
                sample_metadata::Format::Wav => 0,
                sample_metadata::Format::Aiff => 1,
                sample_metadata::Format::Flac => 7,
            });
            result["metadata_channels"] = json!(metadata.channels);
            result["metadata_bits"] = json!(metadata.bits);
            result["metadata_frames"] = json!(metadata.frames);
            result["base_note"] = json!(metadata.base_note);
            result["fine_tune"] = json!(metadata.fine_tune);
            result["tempo"] = json!(metadata.tempo);
            result["geometry_error_code"] = json!(metadata.geometry_error.map(error_code));
            result["normalized_file_loop"] = json!(metadata.looping.map(|looping| {
                let (shape, crossfade) = match looping.shape {
                    LoopShape::Wrap => (0, None), LoopShape::PingPong => (1, None),
                    LoopShape::Crossfade { frames } => (2, Some(frames)),
                    LoopShape::EqualPowerCrossfade { frames } => (3, Some(frames)),
                };
                json!({"start":looping.start,"end_exclusive":looping.end,
                    "mode_code":match looping.mode { LoopMode::Continuous=>0, LoopMode::UntilRelease=>1 },
                    "shape_code":shape,"crossfade_frames":crossfade,
                    "passes":looping.passes.map(|passes|passes.get())})
            }));
            result["header_metadata_channels_equal"] = json!(metadata.channels
                .map(usize::from).zip(channels).map(|(a,b)|a==b));
        }
    }
    if let (Some(budget), Some(encoded)) = (budget.as_deref_mut(), encoded_for_decode) {
        if rate != Some(SOURCE_RATE) || channels != Some(SOURCE_CHANNELS)
            || frames as u64 != SOURCE_FRAMES
            || result["status"] != "metadata_observed"
            || result["metadata_channels"] != SOURCE_CHANNELS
            || result["metadata_bits"] != 16
            || result["base_note"] != 64
        {
            result["status"] = json!("numeric_payload_geometry_refused");
            return result;
        }
        let plan = match numeric_preflight(frames as u64, SOURCE_CHANNELS, SOURCE_RATE, budget) {
            Ok(plan) => plan,
            Err(code) => {
                result["status"] = json!("numeric_preflight_refused");
                result["numeric_refusal_code"] = json!(code);
                return result;
            }
        };
        // EXACT existing decoder, not a format/packet/reader implementation.
        // It decodes the immutable whole file. The 64MiB check above admits
        // declared logical PCM; its unchanged internal limit is 512MiB.
        result["pcm_decoder_invoked"] = json!(true);
        let (samples, decoded_channels, decoded_rate) = match audio::decode_channels(&encoded) {
            Ok(decoded) => decoded,
            Err(_) => {
                result["status"] = json!("existing_pcm_decoder_refused");
                return result;
            }
        };
        result["pcm_decoded"] = json!(true);
        result["decoded_frames"] = json!(samples.len() / decoded_channels.max(1));
        result["decoded_channels"] = json!(decoded_channels);
        result["decoded_rate"] = json!(decoded_rate);
        if decoded_channels != SOURCE_CHANNELS || decoded_rate != SOURCE_RATE
            || samples.len() as u64 != plan.declared_samples
        {
            result["status"] = json!("decoded_geometry_mismatch");
            return result;
        }
        let windows = match analyze_windows(&samples, decoded_channels, decoded_rate, &plan) {
            Ok(windows) => windows,
            Err(code) => {
                result["status"] = json!("numeric_analysis_refused");
                result["numeric_refusal_code"] = json!(code);
                return result;
            }
        };
        result["numeric_windows"] = json!({"status":"observed","pcm_exported":false,
            "clock":"original_source_frames_at_encoded_rate",
            "source_sample_rate":decoded_rate,"file_metadata_base_note":64,
            "runtime_recipe_base_note_context":60,"runtime_note_key_context":60,
            "runtime_output_rate_context":48000,
            "clock_relation":"no_note_rate_or_transposition_applied_to_source_hz",
            "declared_logical_decoded_bytes":plan.decoded_bytes,
            "decoded_logical_budget_bytes":MAX_DECODED_BYTES,
            "allocator_or_rss_bound_claimed":false,
            "existing_full_decoder_internal_limit_bytes":512u64<<20,
            "correlation_sample_pairs_charged":plan.correlation_pairs,
            "correlation_sample_pair_budget":MAX_CORRELATION_PAIRS,
            "correlation_slice_frames_limit":CORRELATION_FRAMES,
            "periodicity_scope":"positive_autocorrelation_candidates_not_tonal_causality_or_fundamental_proof",
            "windows":windows});
    }
    result
}

fn run(request: Request) -> Result<Value, &'static str> {
    let translated = translate_path(&Path::new(BANK).join(MEMBER))
        .map_err(|_| "ordinary_translation_refused")?;
    let matches: Vec<_> = translated.locations.iter().enumerate()
        .filter(|(_, location)| location.len() == WORKER_BYTES && hash(location.as_bytes()) == WORKER_SHA)
        .map(|(asset, _)| asset).collect();
    let runtime_asset = match matches.as_slice() {
        [asset] => *asset,
        [] => return Err("worker_location_hash_join_missing"),
        _ => return Err("worker_location_hash_join_ambiguous"),
    };
    let identity = |asset: usize| -> Result<Value, &'static str> {
        let location = translated.locations.get(asset).ok_or("candidate_asset_absent")?;
        Ok(json!({"asset":asset,"location_sha256":hash(location.as_bytes()),
            "location_utf8_bytes":location.len(),"runtime_location_join":asset==runtime_asset}))
    };
    let mut selected = request.selected;
    if request.runtime { selected.insert(runtime_asset); }
    if selected.len() > 4 { return Err("more_than_four_selected_assets"); }
    if request.numeric_windows && (selected.len()!=1 || !selected.contains(&runtime_asset)) {
        return Err("numeric_windows_need_single_joined_runtime_asset");
    }
    let mut result = json!({"schema":1,"bank_sha256_root_attested":BANK_SHA,
        "runtime_location":identity(runtime_asset)?,"asset_count":translated.locations.len(),
        "scope":"ordinary_saved_translation_and_original_encoded_header_metadata_only",
        "lua_initialized":false,"pcm_decoded":false,"native_or_ui_invoked":false,
        "encoded_byte_budget":request.byte_limit,"max_selected_assets":4,
        "raw_source_or_paths_exported":false});
    if matches!(request.mode, Mode::Join) {
        result["status"] = json!("hash_join_observed_no_encoded_resource_read");
        result["candidate_assets"] = Value::Array(CANDIDATE_ASSETS.into_iter()
            .map(identity).collect::<Result<Vec<_>,_>>()?);
        result["encoded_assets_read"] = json!(0);
        return Ok(result);
    }
    if selected.is_empty() { return Err("explicit_asset_selection_required"); }
    // Resolve every selected index/identity before the first resource payload.
    let mut selected_identities = selected.iter().map(|asset| identity(*asset))
        .collect::<Result<Vec<_>,_>>()?;
    let bank = Bank::open(Path::new(BANK)).map_err(|_| "ordinary_bank_open_refused")?;
    let mut total = 0u64;
    let mut numeric_budget = NumericBudget::default();
    for (asset, row) in selected.iter().zip(&mut selected_identities) {
        let location = translated.locations.get(*asset).ok_or("selected_asset_absent")?;
        let remaining = request.byte_limit.checked_sub(total).ok_or("byte_budget_exhausted")?;
        // NEW narrow normal Bank API preflights all member sizes/count before
        // invoking its unchanged ordinary read/decryption path.
        let parts = bank.resource_bounded(MEMBER, location, remaining)
            .map_err(|_| "bounded_ordinary_resource_refused")?;
        let bytes = parts.iter().try_fold(0u64, |sum, part|
            sum.checked_add(part.len() as u64)).ok_or("encoded_size_overflow")?;
        total = total.checked_add(bytes).ok_or("encoded_size_overflow")?;
        if total > request.byte_limit { return Err("bounded_reader_contract_violation"); }
        row["part_count"] = json!(parts.len());
        row["encoded_bytes"] = json!(bytes);
        if request.numeric_windows && parts.len()!=1 {
            return Err("numeric_windows_need_single_admitted_part");
        }
        row["parts"] = Value::Array(parts.into_iter().enumerate()
            .map(|(part, bytes)| inspect_part(bytes, part,
                request.numeric_windows.then_some(&mut numeric_budget))).collect());
    }
    let refused_parts = selected_identities.iter().filter_map(|row|row["parts"].as_array())
        .flatten().filter(|part|part["status"] != "metadata_observed").count();
    let decoder_invoked = selected_identities.iter().filter_map(|row|row["parts"].as_array())
        .flatten().any(|part|part["pcm_decoder_invoked"] == true);
    let decoded = selected_identities.iter().filter_map(|row|row["parts"].as_array())
        .flatten().any(|part|part["pcm_decoded"] == true);
    result["pcm_decoded"] = json!(decoded);
    result["scope"] = json!(if request.numeric_windows {
        "ordinary_saved_translation_original_channel_numeric_source_windows_no_note_playback"
    } else { "ordinary_saved_translation_and_original_encoded_header_metadata_only" });
    result["status"] = json!(if request.numeric_windows && refused_parts == 0 {
        "bounded_original_channel_numeric_windows_observed"
    } else if request.numeric_windows { "bounded_original_channel_numeric_windows_partial" }
    else if refused_parts == 0 {
        "bounded_original_resource_metadata_observed"
    } else { "bounded_original_resource_metadata_partial" });
    result["metadata_parts_refused"] = json!(refused_parts);
    result["selected_assets"] = Value::Array(selected_identities);
    result["encoded_bytes_read"] = json!(total);
    result["no_pcm_decoder_constructed"] = json!(!decoder_invoked);
    Ok(result)
}

fn main() {
    match request().and_then(run) {
        Ok(result) => println!("{result}"),
        Err(code) => {
            println!("{}", json!({"schema":1,"status":"refused","code":code,
                "raw_source_or_paths_exported":false,"pcm_decoded":false}));
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod numeric_tests {
    use super::*;

    #[test]
    fn original_channels_keep_sine_dc_and_silence_separate() {
        let rate = 48000u32;
        let mut samples = Vec::with_capacity(4096*3);
        for frame in 0..4096 {
            let signal = 0.25+0.125*(frame as f64*480.*std::f64::consts::TAU/f64::from(rate)).sin();
            samples.extend([0.,-0.5,signal as f32]);
        }
        let window = WindowFrames {start:0,end:4096};
        let silent = channel_metrics(&samples,3,0,rate,window).unwrap();
        let dc = channel_metrics(&samples,3,1,rate,window).unwrap();
        let tone = channel_metrics(&samples,3,2,rate,window).unwrap();
        assert_eq!(silent["peak"],0.);
        assert_eq!(silent["ac_rms"],0.);
        assert!(silent["ac_rms_dbfs"].is_null());
        assert_eq!(silent["periodicity"]["status"],"silent_or_dc");
        assert_eq!(dc["dc"],-0.5);
        assert_eq!(dc["peak"],0.5);
        assert_eq!(dc["ac_rms"],0.);
        assert!(dc["periodicity"]["peaks"].as_array().unwrap().is_empty());
        assert!((tone["dc"].as_f64().unwrap()-0.25).abs()<0.001);
        assert!((tone["ac_rms"].as_f64().unwrap()-0.125/2f64.sqrt()).abs()<0.001);
        assert!(tone["periodicity"]["peaks"].as_array().unwrap().iter().any(|peak|
            (peak["source_hz"].as_f64().unwrap()-480.).abs()<1.
                && peak["normalized_correlation"].as_f64().unwrap()>0.99));
        assert_eq!(tone["periodicity"]["lag_strictly_below_half_slice"],true);
    }

    #[test]
    fn original_channel_nonfinite_and_too_short_correlation_are_refused() {
        let window = WindowFrames {start:0,end:2048};
        let mut samples = vec![0f32;2048];
        samples[500] = f32::NAN;
        assert_eq!(channel_metrics(&samples,1,0,44100,window).unwrap_err(),
            "nonfinite_original_channel");
        assert_eq!(channel_metrics(&vec![0.;1024],1,0,44100,
            WindowFrames {start:0,end:1024}).unwrap_err(),
            "correlation_lag_not_below_half_slice");
    }

    #[test]
    fn numeric_preflight_checks_overflow_geometry_and_logical_memory() {
        let mut budget = NumericBudget::default();
        for (frames,channels,rate,expected) in [
            (u64::MAX,12,44100,"decoded_size_overflow"),
            (100000,0,44100,"invalid_numeric_geometry"),
            (100000,13,44100,"invalid_numeric_geometry"),
            (100000,1,0,"invalid_numeric_geometry"),
            (1000,1,44100,"source_window_unavailable"),
            (16000,1,8000,"correlation_lag_not_below_half_slice"),
            (MAX_DECODED_BYTES/4+1,1,44100,"decoded_logical_budget"),
        ] {
            assert_eq!(numeric_preflight(frames,channels,rate,&mut budget).unwrap_err(),expected);
            assert_eq!(budget,NumericBudget::default());
        }
        let plan = numeric_preflight(MAX_DECODED_BYTES/4,1,44100,&mut budget).unwrap();
        assert_eq!(plan.decoded_bytes,MAX_DECODED_BYTES);
        let before = budget;
        assert_eq!(numeric_preflight(100000,1,44100,&mut budget).unwrap_err(),"decoded_logical_budget");
        assert_eq!(budget,before);
    }

    #[test]
    fn numeric_preflight_charges_all_channels_and_windows_before_decode() {
        let mut budget = NumericBudget::default();
        let plan = numeric_preflight(SOURCE_FRAMES,SOURCE_CHANNELS,SOURCE_RATE,&mut budget).unwrap();
        assert_eq!(plan.decoded_bytes,4_716_240);
        assert_eq!(plan.windows.iter().map(|w|(w.start,w.end)).collect::<Vec<_>>(),
            [(0,5512),(11025,22050),(33075,44100),(77175,88200)]);
        assert!(plan.correlation_pairs < MAX_CORRELATION_PAIRS);
        let mut nearly_full = NumericBudget {decoded_bytes:0,
            correlation_pairs:MAX_CORRELATION_PAIRS-1};
        let before = nearly_full;
        assert_eq!(numeric_preflight(SOURCE_FRAMES,SOURCE_CHANNELS,SOURCE_RATE,
            &mut nearly_full).unwrap_err(),"correlation_work_budget");
        assert_eq!(nearly_full,before);
    }
}
