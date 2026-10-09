//! Aggregate corpus metadata only; never emit preset contents or access data.
//! Usage: format_survey <newline-separated paths> <output directory>
use ni_file::kontakt::{Chunk as OwnedChunk, StructuredObject};
use sampler_kontakt::{Chunks, Limits};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Seek, SeekFrom, Write},
    path::Path,
};

const LIMITS: Limits = Limits {
    bytes: 256 << 20,
    records: 1_000_000,
};

#[derive(Default)]
struct Count {
    files: usize,
    changed: usize,
    records: usize,
    last: Option<usize>,
    last_changed: Option<usize>,
}

impl Count {
    fn add(&mut self, file: usize, changed: bool) {
        self.records += 1;
        if self.last != Some(file) {
            self.files += 1;
            self.last = Some(file);
        }
        if changed && self.last_changed != Some(file) {
            self.changed += 1;
            self.last_changed = Some(file);
        }
    }
}

#[derive(Default)]
struct Survey {
    fields: BTreeMap<(String, String, String), Count>,
    lanes: BTreeMap<(String, String, String), Vec<Count>>,
    errors: BTreeMap<String, Count>,
    file: usize,
    metadata_only: bool,
}

impl Survey {
    fn field(&mut self, id: &str, version: &str, field: &str, changed: bool) {
        self.fields
            .entry((id.into(), version.into(), field.into()))
            .or_default()
            .add(self.file, changed);
    }
    fn blob(&mut self, id: &str, version: &str, field: &str, data: &[u8], lanes: bool) {
        self.field(id, version, field, data.iter().any(|&b| b != 0));
        // Byte positions are observations, not invented semantic fields. Variable
        // length text/password/access properties deliberately have no lane profile.
        if lanes {
            let profile = self
                .lanes
                .entry((id.into(), version.into(), field.into()))
                .or_default();
            let count = data.len().min(4096);
            profile.resize_with(profile.len().max(count), Count::default);
            for (c, b) in profile.iter_mut().zip(&data[..count]) {
                c.add(self.file, *b != 0);
            }
            if data.len() > 4096 {
                self.field(
                    id,
                    version,
                    &format!("{field}[4096..]"),
                    data[4096..].iter().any(|&b| b != 0),
                );
            }
        }
    }
    fn error(&mut self, what: &str) {
        self.errors
            .entry(what.into())
            .or_default()
            .add(self.file, true);
    }
    fn object(&mut self, id: u16, object: sampler_kontakt::Structured<'_>, depth: usize) {
        let key = format!("Kontakt:0x{id:02x}");
        let version = format!("0x{:x}", object.version);
        self.field(&key, &version, "structure", true);
        if self.metadata_only && matches!(id, 3 | 0x28 | 0x29) {
            match object.children(LIMITS) {
                Ok(children) => self.chunks(children, depth + 1),
                Err(_) => self.error("metadata children framing"),
            }
            return;
        }
        self.blob(&key, &version, "private", object.private.data(), id != 6);
        self.blob(
            &key,
            &version,
            "public",
            object.public.data(),
            !matches!(id, 3 | 4 | 6 | 0x28 | 0x29),
        );
        // Numeric public fields from the installed decoder; no strings are emitted.
        let owned = OwnedChunk {
            id,
            data: object.raw().data().to_vec(),
        };
        self.scalars(&key, &version, &owned);
        if id == 4 {
            if let Ok(rack) =
                ni_file::kontakt::objects::Group(StructuredObject::try_from(&owned).unwrap())
                    .insert_fx()
            {
                for child in rack.items.iter().flatten() {
                    self.owned(child, depth + 1);
                }
            } else {
                self.error("group private FX rack");
            }
        }
        if id == 0x4f {
            match ni_file::kontakt::objects::Snapshot::try_from(&owned) {
                Ok(snapshot) => {
                    self.field(&key, &version, "group_count", snapshot.group_count != 0);
                    for (slot, entries) in snapshot.persistent.iter().enumerate() {
                        self.field(
                            &key,
                            &version,
                            &format!("persistent_slot_{slot}"),
                            !entries.is_empty(),
                        );
                    }
                    for child in &snapshot.effect_children {
                        self.owned(child, depth + 1);
                    }
                    match snapshot.group_snapshots() {
                        Ok(groups) => {
                            for (_, group) in groups {
                                let v = format!("0x{:x}", group.version);
                                self.field("Kontakt:0x50", &v, "structure", true);
                                self.blob("Kontakt:0x50", &v, "public", &group.public_data, true);
                                self.blob("Kontakt:0x50", &v, "source", &group.source_data, true);
                                self.blob(
                                    "Kontakt:0x50",
                                    &v,
                                    "trailing",
                                    &group.trailing_data,
                                    true,
                                );
                                self.field(
                                    "Kontakt:0x50",
                                    &v,
                                    "trailing_flag",
                                    group.trailing_flag != 0,
                                );
                                for array in [&group.fx, &group.internal, &group.external] {
                                    for child in array.items.iter().flatten() {
                                        self.owned(child, depth + 1);
                                    }
                                }
                            }
                        }
                        Err(_) => self.error("compact snapshot groups"),
                    }
                }
                Err(_) => self.error("snapshot state"),
            }
        }
        match object.children(LIMITS) {
            Ok(children) => self.chunks(children, depth + 1),
            Err(_) => self.error(&format!("{key} children framing")),
        }
    }
    fn owned(&mut self, chunk: &OwnedChunk, depth: usize) {
        let mut data = Vec::new();
        chunk.write(&mut data).unwrap();
        if let Ok(chunks) = Chunks::parse(&data, LIMITS) {
            self.chunks(chunks, depth);
        }
    }
    fn chunks(&mut self, chunks: Chunks<'_>, depth: usize) {
        if depth > 32 {
            self.error("chunk depth limit");
            return;
        }
        for chunk in chunks.iter() {
            if self.metadata_only
                && !matches!(chunk.id, 3 | 0x28 | 0x29 | 0x36 | 0x37 | 0x47 | 0x4b)
            {
                continue;
            }
            let id = format!("Kontakt:0x{:02x}", chunk.id);
            match chunk.id {
                0x33 | 0x34 => match chunk.records(LIMITS) {
                    Ok(records) => {
                        self.field(&id, "list", "structure", !records.is_empty());
                        for record in records.iter() {
                            self.object(
                                if chunk.id == 0x33 { 4 } else { 0x2c },
                                record.object,
                                depth,
                            );
                        }
                    }
                    Err(_) => self.error(&format!("{id} list framing")),
                },
                0x36 => match sampler_kontakt::ProgramList::parse(chunk, LIMITS) {
                    Ok(list) => {
                        self.field(&id, "list", "structure", !list.0.is_empty());
                        for (number, object) in list.0 {
                            self.field(&id, "list", "program_number", number != 0);
                            self.owned(
                                &OwnedChunk {
                                    id: 0x28,
                                    data: object.raw().data().to_vec(),
                                },
                                depth,
                            );
                        }
                    }
                    Err(_) => self.error("program list framing"),
                },
                0x37 => match sampler_kontakt::SlotList::parse(chunk, LIMITS) {
                    Ok(list) => {
                        self.field(&id, "mask64", "structure", !list.0.is_empty());
                        for (_, child) in list.0 {
                            if let Ok(object) = child.structured() {
                                self.object(child.id, object, depth);
                            }
                        }
                    }
                    Err(_) => self.error("slot list framing"),
                },
                0x39 => match sampler_kontakt::Loops::parse(chunk, LIMITS) {
                    Ok(list) => {
                        self.field(
                            &id,
                            "mask8",
                            "structure",
                            list.slots().iter().any(Option::is_some),
                        );
                        for lp in list.slots().iter().flatten() {
                            for (field, changed) in [
                                ("mode", lp.mode != 0),
                                ("start", lp.start != 0),
                                ("length", lp.length != 0),
                                ("count", lp.count != 0),
                                ("alternating", lp.alternating),
                                ("tune", lp.tune != 1.0),
                                ("crossfade", lp.crossfade != 0),
                            ] {
                                self.field("Kontakt:0x05", "0x60", field, changed);
                            }
                            if let Some(object) = lp.object {
                                self.object(5, object, depth);
                            } else {
                                self.field("Kontakt:0x05", "0x60", "structure", true);
                            }
                        }
                    }
                    Err(_) => self.error("loop array framing"),
                },
                0x3a | 0x3b | 0x3c => {
                    let version = chunk
                        .body
                        .data()
                        .get(1..3)
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .unwrap_or(0);
                    self.field(&id, &format!("0x{version:x}"), "structure", true);
                    // Open all occupied native slots, including bypassed modules.
                    let count = if version == 0x13 && chunk.id == 0x3c {
                        chunk
                            .body
                            .data()
                            .get(3..7)
                            .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                            .unwrap_or(0)
                    } else if chunk.id == 0x3a {
                        8
                    } else if chunk.id == 0x3b {
                        16
                    } else {
                        32
                    };
                    let owned = OwnedChunk {
                        id: 0x3a,
                        data: chunk.body.data().to_vec(),
                    };
                    match ni_file::kontakt::objects::BParamArrayBParFX8::read(
                        Cursor::new(owned.data),
                        count.min(64),
                    ) {
                        Ok(array) => {
                            for child in array.items.iter().flatten() {
                                self.owned(child, depth + 1);
                            }
                        }
                        Err(_) => self.error(&format!("{id} parameter array")),
                    }
                }
                0x4b => match sampler_kontakt::FileTable::parse(chunk, LIMITS) {
                    Ok(table) => {
                        let v = table.version.to_string();
                        self.field(&id, &v, "structure", true);
                        self.field(&id, &v, "special_files", !table.special.is_empty());
                        self.field(&id, &v, "sample_files", !table.samples.is_empty());
                        self.field(&id, &v, "other_files", !table.other.is_empty());
                        for file in table
                            .special
                            .iter()
                            .chain(table.other.iter())
                            .chain(table.samples.iter().chain(&table.flat).map(|s| &s.filename))
                        {
                            for segment in file.segments() {
                                self.field(
                                    &id,
                                    &v,
                                    &format!("segment_kind_{}", segment.kind),
                                    true,
                                );
                            }
                        }
                        for sample in table.samples.iter().chain(&table.flat) {
                            if let Some(value) = sample.timestamp {
                                self.field(&id, &v, "sample_timestamp", value != 0);
                            }
                            if let Some(value) = sample.unknown_record {
                                self.field(&id, &v, "sample_unknown_record", value != 0);
                            }
                            if let Some(bytes) = sample.prefix {
                                self.blob(&id, &v, "flat_prefix", bytes.data(), true);
                            }
                            if let Some(bytes) = sample.suffix {
                                self.blob(&id, &v, "flat_suffix", bytes.data(), true);
                            }
                        }
                        self.blob(&id, &v, "extension", table.extension.data(), true);
                    }
                    Err(_) => self.error("filename table framing"),
                },
                0x38 | 0x3d | 0x35 => self.blob(&id, "raw", "body", chunk.body.data(), false),
                _ => match chunk.structured() {
                    Ok(object) => {
                        if self.metadata_only && chunk.id == 0x28 {
                            let version = format!("0x{:x}", object.version);
                            match sampler_kontakt::ProgramResources::parse(chunk) {
                                Ok(program) => {
                                    for (field, value) in [
                                        (
                                            "container_reference",
                                            program.record.resource_container_filename_ref,
                                        ),
                                        ("filename_ref_1", program.record.filename_ref_1),
                                        (
                                            "terminal_filename_ref",
                                            program.record.terminal_filename_ref,
                                        ),
                                    ] {
                                        if let Some(value) = value {
                                            self.field(&id, &version, field, value != -1);
                                        }
                                    }
                                    for (field, text) in [
                                        ("tail_string_0", &program.record.tail_string_0),
                                        ("tail_string_1", &program.record.tail_string_1),
                                    ] {
                                        if let Some(text) = text {
                                            self.field(&id, &version, field, !text.is_empty());
                                        }
                                    }
                                    for (field, sound) in [
                                        ("sound_data_0", &program.record.sound_data_0),
                                        ("sound_data_1", &program.record.sound_data_1),
                                    ] {
                                        if let Some(sound) = sound {
                                            self.field(&id, &version, field, sound.body.is_some());
                                        }
                                    }
                                }
                                Err(_) => self.error("program resource fields"),
                            }
                        }
                        if self.metadata_only && chunk.id == 3 {
                            let version = format!("0x{:x}", object.version);
                            match sampler_kontakt::Bank::parse(chunk) {
                                Ok(bank) => {
                                    for (field, changed) in [
                                        ("master_volume", bank.volume != 1.0),
                                        ("master_tune", bank.tune != 1.0),
                                        ("master_tempo", bank.tempo != 0),
                                    ] {
                                        self.field(&id, &version, field, changed);
                                    }
                                    self.blob(
                                        &id,
                                        &version,
                                        "extension",
                                        bank.extension.data(),
                                        true,
                                    );
                                }
                                Err(_) => self.error("bank fields"),
                            }
                        }

                        if chunk.id == 0x47 {
                            match sampler_kontakt::SaveSettings::parse(chunk, LIMITS) {
                                Ok(settings) => {
                                    self.field(
                                        &id,
                                        "0x10",
                                        "translated_reference",
                                        settings.translated != u32::MAX,
                                    );
                                    self.field(
                                        &id,
                                        "0x10",
                                        "original_reference",
                                        settings.original != -1,
                                    );
                                    self.field(&id, "0x10", "unknown", settings.unknown != 0);
                                    for (i, flag) in settings.flags.iter().enumerate() {
                                        self.field(&id, "0x10", &format!("flag{i}"), *flag);
                                    }
                                    self.blob(
                                        &id,
                                        "0x10",
                                        "extension",
                                        settings.extension.data(),
                                        true,
                                    );
                                }
                                Err(_) => self.error("save settings fields"),
                            }
                        }
                        if chunk.id == 6 {
                            if let Ok(script) =
                                ni_file::kontakt::objects::BParScript::try_from(&OwnedChunk {
                                    id: 6,
                                    data: chunk.body.data().to_vec(),
                                })
                                .and_then(|s| s.params())
                            {
                                self.field(
                                    &id,
                                    &format!("0x{:x}", object.version),
                                    "linked_script",
                                    script.textfile_name.is_some_and(|n| !n.trim().is_empty()),
                                );
                            }
                        }
                        self.object(chunk.id, object, depth);
                    }
                    Err(_) => {
                        self.field(&id, "unframed", "structure", true);
                        self.blob(&id, "unframed", "body", chunk.body.data(), false);
                    }
                },
            }
        }
    }

    fn nis(&mut self, item: &ni_file::nis::ItemContainer, path: &Path, depth: usize) {
        if depth > 16 {
            self.error("NIS depth limit");
            return;
        }
        self.field(
            "NIS:item",
            "1",
            "header_flags",
            item.header.header_flags != 0,
        );
        self.field("NIS:item", "1", "reserved", item.header.reserved != 0);
        self.blob("NIS:item", "1", "uuid", &item.header.uuid, false);
        for descriptor in &item.child_headers {
            for (field, at) in [("child_index", 0), ("child_domain", 4), ("child_id", 8)] {
                self.field(
                    "NIS:item",
                    "1",
                    field,
                    descriptor[at..at + 4].iter().any(|&b| b != 0),
                );
            }
        }
        self.blob("NIS:item", "1", "trailing", &item.trailing_data, false);
        let mut data = Some(&item.data);
        while let Some(layer) = data {
            let id = format!(
                "NIS:{}:0x{:x}",
                String::from_utf8_lossy(&layer.header.domain_id),
                layer.header.item_id
            );
            let v = layer
                .data
                .get(..4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()).to_string())
                .unwrap_or_else(|| "empty".into());
            let v = match layer.header.item_type() {
                ni_file::nis::ItemType::BNISoundHeader => "BPatchHeaderV42".into(),
                ni_file::nis::ItemType::BNISoundPreset => "inherited".into(),
                _ => v,
            };
            self.field(&id, &v, "structure", true);
            if layer.header.item_type() == ni_file::nis::ItemType::Preset {
                if let Some(factory) = layer.data.get(4) {
                    self.field(&id, &v, "factory", *factory != 0);
                }
                if let Some(app) = layer.data.get(5..9) {
                    self.blob(&id, &v, "authoring_app", app, false);
                }
            }
            if layer.header.item_type() == ni_file::nis::ItemType::PresetChunkItem {
                if let Some(checksum) = layer.data.get(4..8) {
                    self.blob(&id, &v, "auth_checksum", checksum, false);
                }
            }
            // Never profile authorization, encrypted bytes or access properties.
            if layer.header.item_type() == ni_file::nis::ItemType::EncryptionItem {
                let encrypted = layer.data.get(4) == Some(&1);
                self.field(&id, &v, "encrypted", encrypted);
                let key = encrypted
                    .then(|| sampler_kontakt::library_key(path).ok())
                    .flatten();
                if let Ok(enc) = ni_file::nis::EncryptionItem::read_with_key(layer, key.as_deref())
                {
                    if let Ok(inner) = enc.subtree.item() {
                        self.nis(&inner, path, depth + 1);
                    }
                } else {
                    self.error("NIS encrypted subtree");
                }
            }
            if layer.header.item_type() == ni_file::nis::ItemType::AppSpecific {
                if let Ok(app) = ni_file::nis::AppSpecificProperties::try_from(layer) {
                    if let Ok(inner) = app.subtree_item.item() {
                        self.nis(&inner, path, depth + 1);
                    }
                }
            }
            data = layer.inner.as_deref();
        }
        for child in &item.children {
            self.nis(child, path, depth + 1);
        }
    }

    fn scalars(&mut self, id: &str, v: &str, chunk: &OwnedChunk) {
        // Generated scalar list lives below; strings, vectors and access data
        // are excluded. Baselines are documented, not guessed from mode values.
        scalar_fields(self, id, v, chunk);
    }

    fn resource(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("nicnt"))
        {
            let mut file = std::fs::File::open(path)?;
            let mut head = Vec::new();
            Read::by_ref(&mut file)
                .take(4 << 20)
                .read_to_end(&mut head)?;
            let marker = b"/\\ NI FC MTD  /\\";
            let start = head
                .windows(marker.len())
                .enumerate()
                .skip(1)
                .find_map(|(at, b)| (b == marker).then_some(at))
                .ok_or("missing NICNT resource section")?;
            file.seek(SeekFrom::Start(start as u64))?;
            let files = ni_file::file_container::NIFileContainer::read(&mut file)?;
            self.field("NICNT:FileContainer", "unversioned", "structure", true);
            self.field(
                "NICNT:FileContainer",
                "unversioned",
                "member_count",
                !files.items.is_empty(),
            );
            for item in files.items {
                self.field(
                    "NICNT:FileContainer",
                    "unversioned",
                    "member_index",
                    item.index != 0,
                );
                self.field(
                    "NICNT:FileContainer",
                    "unversioned",
                    "member_offset",
                    item.file_start_offset != 0,
                );
                self.field(
                    "NICNT:FileContainer",
                    "unversioned",
                    "member_size",
                    item.file_size != 0,
                );
            }
            // Product XML/access fields in `head` are neither decoded nor output.
        } else {
            let mut file = std::fs::File::open(path)?;
            let mut header = [0; 22];
            file.read_exact(&mut header)?;
            let v = format!("0x{:x}", u16::from_le_bytes([header[4], header[5]]));
            self.field("NKR:root-directory", &v, "structure", true);
            for (field, at) in [
                ("set_id", 6),
                ("unknown", 10),
                ("count", 14),
                ("padding", 18),
            ] {
                self.field(
                    "NKR:root-directory",
                    &v,
                    field,
                    header[at..at + 4].iter().any(|&b| b != 0),
                );
            }
            file.rewind()?;
            let archive = ni_file::nkr::Archive::read_index(&mut file)?;
            self.field(
                "NKR:root-directory",
                &v,
                "index_issues",
                !archive.issues.is_empty(),
            );
            // Header offsets preserve disk locality; HashMap iteration makes
            // a whole-archive census needlessly seek across sample payloads.
            let mut members: Vec<_> = archive.members().collect();
            members.sort_unstable_by_key(|entry| entry.header_offset);
            for indexed in members {
                if let Some(entry) = archive.member(&mut file, &indexed.name)? {
                    file.seek(SeekFrom::Start(entry.header_offset))?;
                    let mut common = [0; 14];
                    file.read_exact(&mut common)?;
                    let version = format!("0x{:x}", u16::from_le_bytes([common[4], common[5]]));
                    let magic = u32::from_le_bytes(common[..4].try_into().unwrap());
                    let kind = format!("NKR:member:0x{magic:x}");
                    self.field(&kind, &version, "structure", true);
                    self.field(&kind, &version, "valid", entry.valid);
                    self.field(&kind, &version, "encoded", entry.encoded);
                    self.field(
                        &kind,
                        &version,
                        "key_index_not_clear",
                        entry.key_index != 0xff,
                    );
                    self.field(&kind, &version, "size", entry.size != 0);
                    self.blob(&kind, &version, "reserved_word", &common[6..10], true);
                }
            }
        }
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let files = std::fs::read_to_string(&args[1])?;
    let out = Path::new(&args[2]);
    std::fs::create_dir_all(out)?;
    let mut survey = Survey::default();
    survey.metadata_only = args.get(3).is_some_and(|s| s == "--metadata-only");
    let mut instruments = Vec::new();
    let mut snapshots = Vec::new();
    let mut status = std::fs::File::create(out.join("files.tsv"))?;
    writeln!(status, "path\tstatus")?;
    for (i, path) in files.lines().enumerate() {
        survey.file = i;
        let path = Path::new(path);
        if path.extension().is_some_and(|e| {
            ["nkr", "nkx", "nicnt"]
                .iter()
                .any(|s| e.eq_ignore_ascii_case(s))
        }) {
            writeln!(
                status,
                "{}\t{}",
                path.display(),
                match survey.resource(path) {
                    Ok(()) => "resource-ok",
                    Err(_) => {
                        survey.error("resource metadata");
                        "resource-error"
                    }
                }
            )?;
            continue;
        }
        if !survey.metadata_only {
            if let Ok(mut file) = std::fs::File::open(path) {
                if let Ok(ni_file::NIFile::NISoundContainer(item)) =
                    ni_file::NIFile::read(&mut file)
                {
                    survey.nis(&item, path, 0);
                }
            }
        }
        match sampler_kontakt::read_chunks(path) {
            Ok(chunks) => {
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nki"))
                {
                    if let Some(Ok(program)) = chunks.program() {
                        if let Ok(params) = program.params() {
                            instruments.push((path.to_owned(), params.name));
                        }
                    }
                }
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("nksn"))
                {
                    snapshots.push((
                        path.to_owned(),
                        chunks.find_first(0x51).and_then(|c| {
                            ni_file::kontakt::objects::snapshot_metadata_names(c).ok()
                        }),
                    ));
                }
                let mut bytes = Vec::new();
                chunks.write(&mut bytes)?;
                match Chunks::parse(&bytes, LIMITS) {
                    Ok(chunks) => {
                        survey.chunks(chunks, 0);
                        writeln!(status, "{}\tok", path.display())?;
                    }
                    Err(_) => writeln!(status, "{}\tchunk-framing-error", path.display())?,
                }
            }
            Err(e) => writeln!(status, "{}\t{:?}", path.display(), e.kind())?,
        }
        if i % 100 == 0 {
            eprintln!("surveyed {} files", i + 1);
        }
    }
    let mut bindings = std::fs::File::create(out.join("snapshot-bindings.tsv"))?;
    writeln!(bindings, "snapshot\tstatus\tbase_instrument")?;
    let mut base_counts = BTreeMap::<std::path::PathBuf, usize>::new();
    for (snapshot, names) in snapshots {
        let Some((name, content)) = names else {
            writeln!(bindings, "{}\tinvalid-metadata\t", snapshot.display())?;
            continue;
        };
        let candidates = snapshot_candidates(&snapshot, &name, &content, &instruments);
        let state = match candidates.len() {
            0 => "unresolved",
            1 => "unique-metadata-match",
            _ => "ambiguous",
        };
        if let [base] = candidates.as_slice() {
            *base_counts.entry((*base).to_owned()).or_default() += 1;
        }
        // Only installed filesystem paths are output, never saved metadata names.
        writeln!(
            bindings,
            "{}\t{state}\t{}",
            snapshot.display(),
            candidates
                .iter()
                .map(|p| p.to_string_lossy())
                .collect::<Vec<_>>()
                .join(" | ")
        )?;
    }
    let mut bases = std::fs::File::create(out.join("snapshot-instruments.tsv"))?;
    writeln!(bases, "instrument\tsnapshots")?;
    for (base, count) in base_counts {
        writeln!(bases, "{}\t{count}", base.display())?;
    }
    let mut fields = std::fs::File::create(out.join("fields.tsv"))?;
    writeln!(
        fields,
        "structure\tversion\tfield\tfiles\tnonbaseline_files\trecords"
    )?;
    for ((id, v, field), c) in survey.fields {
        writeln!(
            fields,
            "{id}\t{v}\t{field}\t{}\t{}\t{}",
            c.files, c.changed, c.records
        )?;
    }
    let mut bytes = std::fs::File::create(out.join("byte-profile.tsv"))?;
    writeln!(
        bytes,
        "structure\tversion\tregion\toffset\tfiles\tnonzero_files\trecords"
    )?;
    for ((id, v, region), profile) in survey.lanes {
        for (offset, c) in profile.iter().enumerate() {
            writeln!(
                bytes,
                "{id}\t{v}\t{region}\t{offset}\t{}\t{}\t{}",
                c.files, c.changed, c.records
            )?;
        }
    }
    let mut errors = std::fs::File::create(out.join("errors.tsv"))?;
    writeln!(errors, "decoder\tfiles\trecords")?;
    for (error, c) in survey.errors {
        writeln!(errors, "{error}\t{}\t{}", c.files, c.records)?;
    }
    Ok(())
}

fn library(path: &Path) -> Option<&std::ffi::OsStr> {
    path.strip_prefix("/mnt/MAIN_STORAGE/Libraries/Kontakt")
        .ok()?
        .components()
        .next()
        .map(|c| c.as_os_str())
}

fn snapshot_candidates<'a>(
    snapshot: &Path,
    name: &str,
    content: &str,
    instruments: &'a [(std::path::PathBuf, String)],
) -> Vec<&'a Path> {
    let basename = content.rsplit(['/', '\\']).next().unwrap_or(content);
    let content = Path::new(basename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(basename);
    let generic = matches!(name, "Kontakt" | "TemplateSnapshot");
    instruments
        .iter()
        .filter(|(path, saved)| {
            library(path) == library(snapshot)
                && if generic {
                    path.file_stem()
                        .is_some_and(|s| s.to_string_lossy().eq_ignore_ascii_case(content))
                } else {
                    saved.eq_ignore_ascii_case(name)
                }
        })
        .map(|(path, _)| path.as_path())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_survey_reads_resource_references_inside_program_lists() {
        let mut public = vec![0; 70]; // empty name/credits, common scalars/categories
        public.extend(3i32.to_le_bytes());
        for text in ["Snapshots", "long path"] {
            let words: Vec<_> = text.encode_utf16().collect();
            public.extend((words.len() as u32).to_le_bytes());
            for word in words {
                public.extend(word.to_le_bytes());
            }
        }
        public.extend((-17i32).to_le_bytes());
        public.extend(6i32.to_le_bytes());
        let mut object = vec![1, 0xa8, 0];
        object.extend(0u32.to_le_bytes());
        object.extend((public.len() as u32).to_le_bytes());
        object.extend(public);
        object.extend(0u32.to_le_bytes());
        let mut list = 1i16.to_le_bytes().to_vec();
        list.extend(7i16.to_le_bytes());
        list.extend(object);
        let mut survey = Survey {
            metadata_only: true,
            ..Default::default()
        };
        survey.owned(
            &OwnedChunk {
                id: 0x36,
                data: list,
            },
            0,
        );
        let count = &survey.fields[&(
            "Kontakt:0x28".into(),
            "0xa8".into(),
            "container_reference".into(),
        )];
        assert_eq!((count.files, count.changed, count.records), (1, 1, 1));
        for field in [
            "filename_ref_1",
            "terminal_filename_ref",
            "tail_string_0",
            "tail_string_1",
        ] {
            let count = &survey.fields[&("Kontakt:0x28".into(), "0xa8".into(), field.into())];
            assert_eq!((count.files, count.changed, count.records), (1, 1, 1));
        }
        assert!(
            !survey
                .fields
                .keys()
                .any(|(_, _, field)| field == "wallpaper_reference")
        );
        assert!(survey.errors.is_empty());
    }
    #[test]
    fn repeated_records_count_each_file_once_including_late_nondefault() {
        let mut c = Count::default();
        c.add(0, false);
        c.add(0, true);
        c.add(0, true);
        c.add(1, false);
        assert_eq!((c.files, c.changed, c.records), (2, 1, 4));
    }
    #[test]
    fn snapshot_binding_keeps_library_identity_and_ambiguity() {
        let root = "/mnt/MAIN_STORAGE/Libraries/Kontakt";
        let instruments = vec![
            (
                Path::new(root).join("A/Instruments/One.nki"),
                "Piano".into(),
            ),
            (
                Path::new(root).join("B/Instruments/Two.nki"),
                "Piano".into(),
            ),
        ];
        let snapshot = Path::new(root).join("A/Snapshots/Preset.nksn");
        assert_eq!(
            snapshot_candidates(&snapshot, "Piano", "", &instruments),
            [instruments[0].0.as_path()]
        );
        assert_eq!(
            snapshot_candidates(&snapshot, "TemplateSnapshot", "old\\One.nki", &instruments),
            [instruments[0].0.as_path()]
        );
        assert!(snapshot_candidates(&snapshot, "Missing", "", &instruments).is_empty());
        let mut duplicate = instruments.clone();
        duplicate.push((Path::new(root).join("A/More/One.nki"), "Piano".into()));
        assert_eq!(
            snapshot_candidates(&snapshot, "Piano", "", &duplicate).len(),
            2
        );
    }
}

fn scalar_fields(s: &mut Survey, id: &str, v: &str, chunk: &OwnedChunk) {
    use ni_file::kontakt::objects::*;
    match chunk.id {
        40 => {
            if let Ok(p) = Program::try_from(chunk).and_then(|x| x.params()) {
                s.field(id, v, "name", !p.name.is_empty());
                s.field(
                    id,
                    v,
                    "num_bytes_samples_total",
                    p.num_bytes_samples_total != 0.0,
                );
                s.field(id, v, "transpose", p.transpose != 0);
                s.field(id, v, "volume", p.volume != 1.0);
                s.field(id, v, "pan", p.pan != 0.0);
                s.field(id, v, "tune", p.tune != 1.0);
                s.field(id, v, "low_velocity", p.low_velocity != 0);
                s.field(id, v, "high_velocity", p.high_velocity != 127);
                s.field(id, v, "low_key", p.low_key != 0);
                s.field(id, v, "high_key", p.high_key != 127);
                s.field(id, v, "default_key_switch", p.default_key_switch != -1);
                s.field(
                    id,
                    v,
                    "dfd_channel_preload_size",
                    p.dfd_channel_preload_size != 0,
                );
                s.field(id, v, "library_id", p.library_id != 0);
                s.field(id, v, "fingerprint", p.fingerprint != 0);
                s.field(id, v, "loading_flags", p.loading_flags != 0);
                s.field(id, v, "group_solo", p.group_solo);
                s.field(id, v, "cat_icon_idx", p.cat_icon_idx != 0);
                s.field(
                    id,
                    v,
                    "instrument_credits",
                    !p.instrument_credits.is_empty(),
                );
                s.field(id, v, "instrument_author", !p.instrument_author.is_empty());
                s.field(id, v, "instrument_url", !p.instrument_url.is_empty());
                s.field(id, v, "instrument_cat1", p.instrument_cat1 != 0);
                s.field(id, v, "instrument_cat2", p.instrument_cat2 != 0);
                s.field(id, v, "instrument_cat3", p.instrument_cat3 != 0);
                s.field(
                    id,
                    v,
                    "resource_container_filename",
                    p.resource_container_filename.is_some_and(|n| n != 0),
                );
                s.field(
                    id,
                    v,
                    "wallpaper_filename",
                    p.wallpaper_filename.is_some_and(|n| n != 0),
                );
            }
        }
        4 => {
            if let Ok(p) = StructuredObject::try_from(chunk)
                .map(Group)
                .and_then(|x| x.params())
            {
                s.field(id, v, "name", !p.name.is_empty());
                s.field(id, v, "volume", p.volume != 1.0);
                s.field(id, v, "pan", p.pan != 0.0);
                s.field(id, v, "tune", p.tune != 1.0);
                s.field(id, v, "key_tracking", p.key_tracking);
                s.field(id, v, "reverse", p.reverse);
                s.field(id, v, "release_trigger", p.release_trigger);
                s.field(
                    id,
                    v,
                    "release_trigger_note_monophonic",
                    p.release_trigger_note_monophonic,
                );
                s.field(id, v, "rls_trig_counter", p.rls_trig_counter != 0);
                s.field(id, v, "midi_channel", p.midi_channel != -1);
                s.field(id, v, "voice_group_index", p.voice_group_index != 0);
                s.field(
                    id,
                    v,
                    "fx_idx_amp_split_point",
                    p.fx_idx_amp_split_point != 0,
                );
                s.field(id, v, "muted", p.muted);
                s.field(id, v, "soloed", p.soloed);
                s.field(id, v, "interp_quality", p.interp_quality != 0);
            }
        }
        44 => {
            if let Ok(p) = StructuredObject::try_from(chunk)
                .map(Zone)
                .and_then(|x| x.params())
            {
                s.field(id, v, "sample_start", p.sample_start != 0);
                s.field(id, v, "sample_end", p.sample_end != 0);
                s.field(
                    id,
                    v,
                    "sample_start_mod_range",
                    p.sample_start_mod_range != -1,
                );
                s.field(id, v, "low_velocity", p.low_velocity != 0);
                s.field(id, v, "high_velocity", p.high_velocity != 127);
                s.field(id, v, "low_key", p.low_key != 0);
                s.field(id, v, "high_key", p.high_key != 127);
                s.field(id, v, "fade_low_velocity", p.fade_low_velocity != 0);
                s.field(id, v, "fade_high_velocity", p.fade_high_velocity != 0);
                s.field(id, v, "fade_low_key", p.fade_low_key != 0);
                s.field(id, v, "fade_high_key", p.fade_high_key != 0);
                s.field(id, v, "root_key", p.root_key != 0);
                s.field(id, v, "zone_volume", p.zone_volume != 1.0);
                s.field(id, v, "zone_pan", p.zone_pan != 0.0);
                s.field(id, v, "zone_tune", p.zone_tune != 1.0);
                s.field(id, v, "filename_id", p.filename_id != 0);
                s.field(id, v, "sample_present", !p.sample_present);
                s.field(id, v, "sample_data_type", p.sample_data_type != 0);
                s.field(id, v, "sample_rate", p.sample_rate != 0);
                s.field(id, v, "num_channels", p.num_channels != 0);
                s.field(id, v, "num_frames", p.num_frames != 0);
                s.field(id, v, "reserved1", p.reserved1 != 0);
                s.field(id, v, "reserved2", p.reserved2.is_some_and(|n| n != 0));
                s.field(id, v, "root_note", p.root_note != 0);
                s.field(id, v, "tuning", p.tuning != 0.0);
                s.field(id, v, "reserved3", p.reserved3 != 0);
                s.field(id, v, "reserved4", p.reserved4 != 0);
            }
        }
        3 => {
            if let Ok(p) = Bank::try_from(chunk).and_then(|x| x.params()) {
                s.field(id, v, "master_volume", p.master_volume != 1.0);
                s.field(id, v, "master_tune", p.master_tune != 1.0);
                s.field(id, v, "master_tempo", p.master_tempo != 0);
                s.field(id, v, "name", !p.name.is_empty());
            }
        }
        37 => {
            if let Ok(p) = BParFX::try_from(chunk).and_then(|x| x.params()) {
                s.field(id, v, "effect_type", p.effect_type != 0);
                s.field(id, v, "bypass", p.bypass);
                s.field(id, v, "output_gain", p.output_gain != 1.0);
                s.field(id, v, "dry_level", p.dry_level != 0.0);
            }
        }
        78 => {
            if let Ok(p) = QuickBrowseData::try_from(chunk).and_then(|x| x.params()) {
                s.field(id, v, "unknown", p.unknown != 0);
            }
        }
        _ => {}
    }
}
