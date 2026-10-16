//! PRIVATE candidate: root registers/builds/runs separately after inspection.
//! Ordinary Bank/translation APIs only; header metadata, no authored execution
//! or PCM decode. Protected paths, file bytes and program text stay in memory.
#[path = "../src/sample_metadata.rs"]
mod sample_metadata;

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
}

fn request() -> Result<Request, &'static str> {
    let mut mode = None;
    let mut attested = false;
    let mut selected = BTreeSet::new();
    let mut runtime = false;
    let mut byte_limit = MAX_BYTES;
    for arg in std::env::args().skip(1) {
        if arg == "--join-only" {
            if mode.is_some() { return Err("duplicate_mode"); }
            mode = Some(Mode::Join);
        } else if arg == "--metadata-only" {
            if mode.is_some() { return Err("duplicate_mode"); }
            mode = Some(Mode::Metadata);
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
    Ok(Request { mode: mode.ok_or("mode_required")?, selected, runtime, byte_limit })
}

fn error_code(error: sample_metadata::MetadataError) -> u8 {
    use sample_metadata::MetadataError as E;
    match error {
        E::Format => 1, E::Truncated => 2, E::Limit => 3, E::Invalid => 4,
        E::UnsupportedVendorSamplerData => 5, E::UnsupportedFractionalLoop => 6,
        E::UnsupportedConflictingLoops => 7,
    }
}

fn inspect_part(bytes: Vec<u8>, ordinal: usize) -> Value {
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
        row["parts"] = Value::Array(parts.into_iter().enumerate()
            .map(|(part, bytes)| inspect_part(bytes, part)).collect());
    }
    let refused_parts = selected_identities.iter().filter_map(|row|row["parts"].as_array())
        .flatten().filter(|part|part["status"] != "metadata_observed").count();
    result["status"] = json!(if refused_parts == 0 {
        "bounded_original_resource_metadata_observed"
    } else { "bounded_original_resource_metadata_partial" });
    result["metadata_parts_refused"] = json!(refused_parts);
    result["selected_assets"] = Value::Array(selected_identities);
    result["encoded_bytes_read"] = json!(total);
    result["no_pcm_decoder_constructed"] = json!(true);
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
