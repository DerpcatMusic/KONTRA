//! Aggregate source-field statistics; never writes preset payloads or sample data.
use ni_file::kontakt::objects::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Default)]
struct Survey {
    rows: BTreeMap<String, (BTreeSet<usize>, BTreeSet<usize>, BTreeMap<String, usize>)>,
}
impl Survey {
    fn field(
        &mut self,
        file: usize,
        object: &str,
        version: u16,
        field: &str,
        value: impl ToString,
        default: impl ToString,
    ) {
        let value = value.to_string();
        let row = self
            .rows
            .entry(format!("{object}\t0x{version:x}\t{field}"))
            .or_default();
        row.0.insert(file);
        if value != default.to_string() {
            row.1.insert(file);
        }
        *row.2.entry(value).or_default() += 1;
    }
    fn program(&mut self, file: usize, p: &Program) -> Result<(), ni_file::Error> {
        let v = p.version();
        self.field(file, "0x28", v, "public_bytes", p.0.public_data.len(), 0);
        self.field(file, "0x28", v, "private_bytes", p.0.private_data.len(), 0);
        let q = p.params()?;
        self.field(
            file,
            "0x28",
            v,
            "unknown_tail_bytes",
            q.unknown_tail.len(),
            0,
        );
        macro_rules! pf { ($($field:ident = $default:expr),* $(,)?) => { $(self.field(file,"0x28",v,stringify!($field),q.$field,$default);)* }; }
        pf!(
            transpose = 0,
            volume = 1.0,
            pan = 0.0,
            tune = 1.0,
            low_velocity = 0,
            high_velocity = 127,
            low_key = 0,
            high_key = 127,
            default_key_switch = -1,
            dfd_channel_preload_size = 0,
            library_id = 0,
            fingerprint = 0,
            loading_flags = 0,
            group_solo = false,
            cat_icon_idx = 0,
            instrument_cat1 = 0,
            instrument_cat2 = 0,
            instrument_cat3 = 0
        );
        self.field(file, "0x28", v, "name_nonempty", !q.name.is_empty(), false);
        self.field(
            file,
            "0x28",
            v,
            "num_bytes_samples_total",
            q.num_bytes_samples_total,
            0.0,
        );
        self.field(
            file,
            "0x28",
            v,
            "credits_nonempty",
            !q.instrument_credits.is_empty(),
            false,
        );
        self.field(
            file,
            "0x28",
            v,
            "author_nonempty",
            !q.instrument_author.is_empty(),
            false,
        );
        self.field(
            file,
            "0x28",
            v,
            "url_nonempty",
            !q.instrument_url.is_empty(),
            false,
        );
        if let Some(c) = p.0.find_first(0x32) {
            self.field(file, "0x32", 0x60, "payload_bytes", c.data.len(), 0);
            let voices = VoiceGroups::try_from(c)?;
            for (object, q) in std::iter::once(("0x32/program", &voices.voice_limit)).chain(
                voices
                    .groups
                    .iter()
                    .flatten()
                    .map(|v| ("0x32/0x2b", &v.voice_limit)),
            ) {
                macro_rules! vf { ($($field:ident = $default:expr),* $(,)?) => { $(self.field(file,object,0x60,stringify!($field),q.$field,$default);)* }; }
                vf!(
                    kill_mode = 1,
                    prefer_released = true,
                    max_num_voices = 1,
                    ms_fade_time = 10,
                    exclusion_group = -1
                );
                self.field(
                    file,
                    object,
                    0x60,
                    "name_nonempty",
                    !q.name.is_empty(),
                    false,
                );
            }
        }
        if let Some(c) = p.0.find_first(0x33) {
            for g in GroupList::try_from(c)?.groups {
                let v = g.0.version;
                let q = g.params()?;
                self.field(
                    file,
                    "0x33/0x04",
                    v,
                    "unknown_tail_bytes",
                    q.unknown_tail.len(),
                    0,
                );
                self.field(
                    file,
                    "0x33/0x04",
                    v,
                    "name_nonempty",
                    !q.name.is_empty(),
                    false,
                );
                self.field(file, "0x38", 0, "mask", q.start_criteria.mask, 0);
                self.field(
                    file,
                    "0x38",
                    0,
                    "unknown_tail_bytes",
                    q.start_criteria.unknown_tail.len(),
                    0,
                );
                self.field(
                    file,
                    "0x33/0x04",
                    v,
                    "public_bytes",
                    g.0.public_data.len(),
                    0,
                );
                self.field(
                    file,
                    "0x33/0x04",
                    v,
                    "private_bytes",
                    g.0.private_data.len(),
                    0,
                );
                macro_rules! gf { ($($field:ident = $default:expr),* $(,)?) => { $(self.field(file,"0x33/0x04",v,stringify!($field),q.$field,$default);)* }; }
                gf!(
                    volume = 1.0,
                    pan = 0.0,
                    tune = 1.0,
                    key_tracking = true,
                    reverse = false,
                    release_trigger = false,
                    release_trigger_note_monophonic = false,
                    rls_trig_counter = 0,
                    midi_channel = -1,
                    voice_group_index = 0,
                    fx_idx_amp_split_point = 0,
                    muted = false,
                    soloed = false,
                    interp_quality = 0
                );
                for s in &q.start_criteria.items {
                    macro_rules! sf { ($($field:ident = $default:expr),* $(,)?) => { $(self.field(file,"0x38/0x0f",0x70,stringify!($field),s.$field,$default);)* }; }
                    sf!(
                        mode = 0,
                        next_criteria = 0,
                        key_min = 0,
                        key_max = 127,
                        controller = 0,
                        cc_min = 0,
                        cc_max = 127,
                        cycle_class = 0,
                        slice_zone_idx = 0,
                        slice_zone_slice_idx = 0,
                        sequencer_only = false
                    );
                }
                if let Ok(s) = g.source_identity() {
                    self.field(file, "0x0e", s.version, "mode", s.mode, 0);
                    self.field(file, "0x0e", s.version, "flag", s.flag, 0);
                    match g.source_params() {
                        Ok((params, tail)) => {
                            self.field(file, "0x0e", s.version, "decoded_bytes", params.bytes, 0);
                            self.field(
                                file,
                                "0x0e",
                                s.version,
                                "private_tail_bytes",
                                tail.len(),
                                0,
                            );
                            for f in params.fields {
                                let name = if f.offset < 30 {
                                    f.name.to_string()
                                } else {
                                    format!("mode_{}/{}", s.mode, f.name)
                                };
                                match f.value {
                                    SourceValue::Float(v) => {
                                        self.field(file, "0x0e", s.version, &name, v, 0.0)
                                    }
                                    SourceValue::Integer(v) => {
                                        self.field(file, "0x0e", s.version, &name, v, 0)
                                    }
                                    SourceValue::Flag(v) => {
                                        self.field(file, "0x0e", s.version, &name, v, false)
                                    }
                                }
                            }
                        }
                        Err(_) => self.field(file, "0x0e", s.version, "decode_error", true, false),
                    }
                }
            }
        }
        if let Some(c) = p.0.find_first(0x34) {
            let z = ZoneList::try_from(c)?;
            for zone in z.zones() {
                let v = zone.0.version;
                let q = zone.params()?;
                self.field(
                    file,
                    "0x34/0x2c",
                    v,
                    "sample_present",
                    q.sample_present,
                    true,
                );
                self.field(file, "0x34/0x2c", v, "filename_id", q.filename_id, -1);
                self.field(
                    file,
                    "0x34/0x2c",
                    v,
                    "unknown_tail_bytes",
                    q.unknown_tail.len(),
                    0,
                );
                if let Some(value) = q.reserved2 {
                    self.field(file, "0x34/0x2c", v, "reserved2", value, 0);
                }
                if let Some(prefix) = q.filename_prefix {
                    self.field(
                        file,
                        "0x34/0x2c",
                        v,
                        "filename_prefix_nonzero",
                        prefix.iter().any(|b| *b != 0),
                        false,
                    );
                }
                self.field(
                    file,
                    "0x34/0x2c",
                    v,
                    "public_bytes",
                    zone.0.public_data.len(),
                    0,
                );
                self.field(
                    file,
                    "0x34/0x2c",
                    v,
                    "private_bytes",
                    zone.0.private_data.len(),
                    0,
                );
                macro_rules! zf { ($($field:ident = $default:expr),* $(,)?) => { $(self.field(file,"0x34/0x2c",v,stringify!($field),q.$field,$default);)* }; }
                zf!(
                    sample_start = 0,
                    sample_end = 0,
                    sample_start_mod_range = 0,
                    low_velocity = 0,
                    high_velocity = 127,
                    low_key = 0,
                    high_key = 127,
                    fade_low_velocity = 0,
                    fade_high_velocity = 0,
                    fade_low_key = 0,
                    fade_high_key = 0,
                    root_key = 60,
                    zone_volume = 1.0,
                    zone_pan = 0.0,
                    zone_tune = 1.0,
                    sample_data_type = 0,
                    sample_rate = 44100,
                    num_channels = 1,
                    num_frames = 0,
                    reserved1 = 0,
                    root_note = 60,
                    tuning = 1.0,
                    reserved3 = 0,
                    reserved4 = 0
                );
                if let Some(c) = zone.0.find_first(0x39) {
                    for l in LoopArray::try_from(c)?.items {
                        macro_rules! lf { ($($field:ident = $default:expr),* $(,)?) => { $(self.field(file,"0x39/0x05",0x60,stringify!($field),l.$field,$default);)* }; }
                        lf!(
                            mode = 0,
                            loop_start = 0,
                            loop_length = 0,
                            loop_count = 0,
                            alternating_loop = false,
                            loop_tuning = 1.0,
                            x_fade_length = 0
                        );
                    }
                }
            }
        }
        Ok(())
    }
}
impl Survey {
    fn write(self, mut out: impl std::io::Write) -> std::io::Result<()> {
        writeln!(
            out,
            "object\tversion\tfield\tfiles_present\tfiles_nondefault\tvalues(value:records)"
        )?;
        for (key, (present, nondefault, values)) in self.rows {
            let values = values
                .iter()
                .map(|(v, n)| format!("{v}:{n}"))
                .collect::<Vec<_>>()
                .join(",");
            writeln!(
                out,
                "{key}\t{}\t{}\t{values}",
                present.len(),
                nondefault.len()
            )?;
        }
        Ok(())
    }
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let list = args.get(1).expect("items.tsv cache-dir [start [count]]");
    let cache = Path::new(args.get(2).expect("cache-dir"));
    let start = args.get(3).map_or(0, |v| v.parse().unwrap());
    let count = args.get(4).map_or(25, |v| v.parse().unwrap());
    std::fs::create_dir_all(cache).unwrap();
    // No parser panic text/preset bytes are logged; only corpus IDs and status.
    std::panic::set_hook(Box::new(|_| {}));
    let began = std::time::Instant::now();
    let list = std::fs::read_to_string(list).unwrap();
    let items: Vec<_> = list
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let (kind, path) = line.split_once('\t')?;
            (kind == "kontakt"
                && matches!(
                    Path::new(path).extension().and_then(|s| s.to_str()),
                    Some("nki" | "nkm")
                ))
            .then_some((i, path))
        })
        .collect();
    for &(i, path) in items.iter().skip(start).take(count) {
        let output = cache.join(format!("{i:04}.tsv"));
        let status = cache.join(format!("{i:04}.status"));
        if status.exists() {
            continue;
        }
        let mut survey = Survey::default();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let chunks = sampler_kontakt::read_chunks(Path::new(path)).map_err(|_| ())?;
            if let Some(c) = chunks.find_first(0x28) {
                let p = Program::try_from(c).map_err(|_| ())?;
                survey.program(i, &p).map_err(|_| ())
            } else {
                let bank = Bank::try_from(chunks.find_first(3).ok_or(())?).map_err(|_| ())?;
                let slots = bank.slot_list().map_err(|_| ())?;
                for container in slots.slots.values() {
                    for p in container.program_list().map_err(|_| ())?.programs {
                        survey.program(i, &p).map_err(|_| ())?;
                    }
                }
                Ok(())
            }
        }));
        let ok = matches!(result, Ok(Ok(())));
        // Retain counts for the successfully read prefix of a failed item.
        survey
            .write(std::fs::File::create(output).unwrap())
            .unwrap();
        std::fs::write(status, if ok { "ok\n" } else { "failed\n" }).unwrap();
        println!("item={i} status={}", if ok { "ok" } else { "failed" });
    }
    println!("shard_seconds={:.3}", began.elapsed().as_secs_f64());
}
