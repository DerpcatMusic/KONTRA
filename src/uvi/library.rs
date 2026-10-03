//! Local UFS resources: exact virtual paths, private keyed reads and shared PCM.
use super::{
    crypto, generator, host,
    program::{self, Program},
    sample::{self, Sample},
    ufs::{Directory, Member, Ufs},
};
use anyhow::{Context, Result, ensure};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    rc::Rc,
    sync::Arc,
};

pub(crate) const PCM_LIMIT: usize = 512 << 20;
const MODULE_LIMIT: usize = 16 << 20;
const ALIAS_LIMIT: usize = 16 << 20;

fn alias_bytes(current: usize, path: &str) -> Result<usize> {
    ensure!(
        !path.is_empty() && path.len() <= 4096 && !path.contains('\0'),
        "Invalid UVI resource alias"
    );
    let total = current
        .checked_add(path.len())
        .and_then(|bytes| {
            bytes.checked_add(std::mem::size_of::<String>() + std::mem::size_of::<Arc<Sample>>())
        })
        .context("UVI resource alias memory overflow")?;
    ensure!(
        total <= ALIAS_LIMIT,
        "UVI resource aliases exceed 16 MiB limit"
    );
    Ok(total)
}

fn normalize(path: &str) -> Result<String> {
    ensure!(
        !path.contains(['\0', ':']) && !path.starts_with('$'),
        "Unresolved UVI volume path"
    );
    let path = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                ensure!(
                    parts.pop().is_some(),
                    "UVI resource path ascends above bank root"
                );
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

/// Exact paths take priority. A bare member name is usable only if unique.
pub fn resolve_member<'a>(directory: &'a Directory, path: &str) -> Result<&'a Member> {
    let path = normalize(path)?;
    let exact: Vec<_> = directory
        .files
        .iter()
        .filter(|m| m.path.as_deref() == Some(&path))
        .collect();
    if exact.len() == 1 {
        return Ok(exact[0]);
    }
    ensure!(exact.is_empty(), "Ambiguous UVI member path");
    ensure!(
        !path.contains('/'),
        "UVI resource path is absent from this bank"
    );
    let named: Vec<_> = directory.files.iter().filter(|m| m.name == path).collect();
    ensure!(
        named.len() == 1,
        "UVI member name must identify exactly one record (found {})",
        named.len()
    );
    Ok(named[0])
}

fn resource<'a>(directory: &'a Directory, program_path: &str, path: &str) -> Result<&'a Member> {
    let base = program_path.rsplit_once('/').map_or("", |(base, _)| base);
    let path = if path.starts_with('/') {
        normalize(path)?
    } else {
        normalize(&format!("{base}/{path}"))?
    };
    resolve_member(directory, &path)
}

/// A starred filename is an ordered list of synchronized mono channels.
fn resources<'a>(
    directory: &'a Directory,
    program_path: &str,
    path: &str,
) -> Result<Vec<&'a Member>> {
    if let Some((base, names)) = path.split_once('*') {
        ensure!(
            base.ends_with(['/', '\\']),
            "Invalid UVI channel bundle prefix"
        );
        let names: Vec<_> = names.split('*').collect();
        ensure!(
            !names.is_empty() && names.len() <= 12,
            "UVI channel bundle exceeds 12 channels"
        );
        names
            .into_iter()
            .map(|name| {
                ensure!(
                    !name.is_empty() && !name.contains(['/', '\\']),
                    "Invalid UVI channel bundle member"
                );
                resource(directory, program_path, &format!("{base}{name}"))
            })
            .collect()
    } else {
        Ok(vec![resource(directory, program_path, path)?])
    }
}

pub struct Library {
    pub bank: Ufs,
    pub directory: Directory,
    content_key: Option<u64>,
}

pub struct LoadedProgram {
    pub program: Program,
    pub path: String,
}

impl Library {
    pub fn open(path: &Path, namespace: &[u8], content_key: Option<u64>) -> Result<Self> {
        let bank = Ufs::open(path)?;
        let directory = bank.decode_directory(namespace)?;
        Ok(Self {
            bank,
            directory,
            content_key,
        })
    }

    pub fn read(&self, member: &Member) -> Result<Vec<u8>> {
        self.bank
            .read_member(member, self.directory.metadata_key, self.content_key)
    }

    pub fn program(&self, name: &str, namespace: &[u8]) -> Result<LoadedProgram> {
        let member = resolve_member(&self.directory, name)?;
        let bytes = self.read(member)?;
        let text = std::str::from_utf8(&bytes).context("Decoded UVI program is not UTF-8")?;
        let text = crypto::decode_program(text, namespace)?;
        Ok(LoadedProgram {
            program: program::parse_program(&text)?,
            path: member
                .path
                .clone()
                .context("UVI program has no decoded directory path")?,
        })
    }

    /// Resolve and decode an approved bank-local audio resource, including bundles.
    pub fn audio(&self, program_path: &str, path: &str) -> Result<Sample> {
        let members = resources(&self.directory, program_path, path)?;
        let mut operands = Vec::with_capacity(members.len());
        for member in members {
            let bytes = self.read(member)?;
            ensure!(
                !generator::image_signature(&bytes),
                "Image wavetable resources cannot be loaded through audio callbacks"
            );
            operands.push(sample::decode(&bytes)?);
        }
        if operands.len() == 1 {
            Ok(operands.pop().unwrap())
        } else {
            sample::assemble_mono(operands)
        }
    }

    /// Initial WaveTableOscillator resources may contain audio or measured images.
    /// The signature, rather than the filename suffix, selects image conversion.
    fn wavetable(&self, program_path: &str, path: &str) -> Result<Sample> {
        let members = resources(&self.directory, program_path, path)?;
        if members.len() != 1 {
            return self.audio(program_path, path);
        }
        let member = members[0];
        ensure!(
            member.size <= generator::IMAGE_BYTES_LIMIT as u64,
            "Wavetable resource exceeds 8 MiB source limit"
        );
        let bytes = self.read(member)?;
        if generator::image_signature(&bytes) {
            generator::image_wavetable(&bytes)
        } else {
            sample::decode(&bytes)
        }
    }

    /// Ordered archive records identify audio independently of path spelling.
    pub(crate) fn audio_identity(&self, program_path: &str, path: &str) -> Result<Vec<u64>> {
        Ok(resources(&self.directory, program_path, path)?
            .iter()
            .map(|member| member.record_offset)
            .collect())
    }

    /// Bounded bank-local data, with the same path authority as audio loading.
    pub fn data(&self, program_path: &str, path: &str, limit: u64) -> Result<Vec<u8>> {
        let member = resource(&self.directory, program_path, path)?;
        ensure!(
            member.size <= limit,
            "UVI resource exceeds caller's byte limit"
        );
        self.read(member)
    }

    /// Source-only modules supplied to the sandbox, with no ambient disk search.
    pub fn modules(&self) -> Result<BTreeMap<String, Vec<u8>>> {
        let mut modules = BTreeMap::new();
        let mut total = 0usize;
        let mut stems = HashMap::<String, Vec<&Member>>::new();
        for member in self
            .directory
            .files
            .iter()
            .filter(|m| m.name.to_ascii_lowercase().ends_with(".lua"))
        {
            ensure!(member.size <= 2 << 20, "UVI Lua module exceeds 2 MiB limit");
            total = total
                .checked_add(member.size as usize)
                .context("UVI module size overflow")?;
            ensure!(total <= MODULE_LIMIT, "UVI modules exceed 16 MiB limit");
            let bytes = self.read(member)?;
            std::str::from_utf8(&bytes).context("Decoded UVI module is not UTF-8")?;
            ensure!(
                !bytes.starts_with(b"\x1bLua"),
                "Only Lua source modules are accepted"
            );
            let path = member
                .path
                .as_ref()
                .context("UVI module has no decoded directory path")?;
            let name = path[..path.len() - 4].replace('/', ".");
            ensure!(
                modules.insert(name, bytes).is_none(),
                "Duplicate UVI module path"
            );
            stems
                .entry(member.name[..member.name.len() - 4].into())
                .or_default()
                .push(member);
        }
        for (stem, members) in stems {
            if members.len() == 1 && !modules.contains_key(&stem) {
                let path = members[0].path.as_ref().unwrap();
                let bytes = &modules[&path[..path.len() - 4].replace('/', ".")];
                total = total
                    .checked_add(bytes.len())
                    .context("UVI module size overflow")?;
                ensure!(
                    total <= MODULE_LIMIT,
                    "UVI modules exceed 16 MiB limit including aliases"
                );
                modules.insert(stem, bytes.clone());
            }
        }
        Ok(modules)
    }

    /// Decode each referenced sample once, retaining every source channel.
    /// Bypassed oscillators still need their resources if Lua enables them.
    pub fn samples(&self, loaded: &LoadedProgram) -> Result<HashMap<String, Arc<Sample>>> {
        let mut result = HashMap::<String, Arc<Sample>>::new();
        let mut cache = HashMap::<Vec<u64>, Arc<Sample>>::new();
        let mut total = 0usize;
        let paths = loaded
            .program
            .sample_zones
            .iter()
            .map(|z| (z.sample_path.as_str(), false))
            .chain(loaded.program.nodes.iter().filter_map(|n| {
                match n.kind.as_str() {
                    "Convolver" | "SampledReverb" => n
                        .attributes
                        .get("SamplePath")
                        .map(|path| (path.as_str(), false)),
                    "WaveTableOscillator" => n
                        .attributes
                        .get("WavetablePath")
                        .map(|path| (path.as_str(), true)),
                    _ => None,
                }
            }));
        for (path, wavetable) in paths.filter(|(p, _)| !p.is_empty()) {
            if let Some(sample) = result.get(path) {
                ensure!(
                    wavetable || !sample.wavetable_image,
                    "Image wavetable resources cannot be used as audio"
                );
                continue;
            }
            let identity = self.audio_identity(&loaded.path, path)?;
            let sample = if let Some(sample) = cache.get(&identity) {
                ensure!(
                    wavetable || !sample.wavetable_image,
                    "Image wavetable resources cannot be used as audio"
                );
                sample.clone()
            } else {
                let decoded = if wavetable {
                    self.wavetable(&loaded.path, path)?
                } else {
                    self.audio(&loaded.path, path)?
                };
                total = total
                    .checked_add(decoded.interleaved.bytes())
                    .context("UVI sample memory overflow")?;
                ensure!(
                    total <= PCM_LIMIT,
                    "UVI resident sample data exceeds 512 MiB limit"
                );
                let sample = Arc::new(decoded);
                cache.insert(identity, sample.clone());
                sample
            };
            result.insert(path.to_owned(), sample);
        }
        Ok(result)
    }
}

/// Decoded bank resources owned by a single Lua/playback worker.
/// The callback performs disk I/O and decoding; it must run off the audio thread.
pub struct BankResources {
    samples: Rc<RefCell<HashMap<String, Arc<Sample>>>>,
    capability: host::Resources,
    revision: Rc<Cell<u64>>,
}

impl BankResources {
    pub fn new(
        library: Rc<Library>,
        program_path: &str,
        initial: HashMap<String, Arc<Sample>>,
    ) -> Result<Self> {
        let samples = Rc::new(RefCell::new(initial));
        let aliases = Cell::new(
            samples
                .borrow()
                .keys()
                .try_fold(0, |bytes, path| alias_bytes(bytes, path))?,
        );
        let revision = Rc::new(Cell::new(0u64));
        let mut identities = HashSet::new();
        let resident = Rc::new(Cell::new(
            samples
                .borrow()
                .values()
                .filter(|sample| identities.insert(Arc::as_ptr(sample)))
                .map(|sample| sample.interleaved.bytes())
                .sum::<usize>(),
        ));
        ensure!(
            resident.get() <= PCM_LIMIT,
            "UVI loaded audio exceeds resident PCM limit"
        );
        let mut audio_cache = HashMap::new();
        for (path, sample) in samples.borrow().iter() {
            if !sample.wavetable_image {
                audio_cache.insert(library.audio_identity(program_path, path)?, sample.clone());
            }
        }
        let capability: host::Resources = {
            let library = library.clone();
            let cache = samples.clone();
            let program_path = program_path.to_owned();
            let audio_cache = RefCell::new(audio_cache);
            let revision = revision.clone();
            Rc::new(move |request| {
                let resolve = || -> Result<host::ResourceResponse> {
                    match request {
                        host::ResourceRequest::ReadAudio { path, .. } => {
                            let existing = cache.borrow().get(path).cloned();
                            let sample = if let Some(sample) = existing {
                                ensure!(
                                    !sample.wavetable_image,
                                    "Image wavetable resources cannot be loaded through audio callbacks"
                                );
                                sample
                            } else {
                                let alias_total = alias_bytes(aliases.get(), path)?;
                                let next_revision = revision
                                    .get()
                                    .checked_add(1)
                                    .context("UVI resource revision exhausted")?;
                                let identity = library.audio_identity(&program_path, path)?;
                                let existing = audio_cache.borrow().get(&identity).cloned();
                                let sample = if let Some(sample) = existing {
                                    sample
                                } else {
                                    let decoded = library.audio(&program_path, path)?;
                                    let total = resident
                                        .get()
                                        .checked_add(decoded.interleaved.bytes())
                                        .context("UVI resource memory overflow")?;
                                    ensure!(
                                        total <= PCM_LIMIT,
                                        "UVI loaded audio exceeds resident PCM limit"
                                    );
                                    let sample = Arc::new(decoded);
                                    audio_cache.borrow_mut().insert(identity, sample.clone());
                                    resident.set(total);
                                    sample
                                };
                                cache.borrow_mut().insert(path.clone(), sample.clone());
                                aliases.set(alias_total);
                                revision.set(next_revision);
                                sample
                            };
                            Ok(host::ResourceResponse::Audio(host::ResourceInfo {
                                name: path
                                    .replace('\\', "/")
                                    .rsplit('/')
                                    .next()
                                    .unwrap_or(path)
                                    .to_owned(),
                                rate: sample.rate,
                                channels: sample.channels,
                                frames: sample.frames,
                            }))
                        }
                        host::ResourceRequest::ReadData { path }
                        | host::ResourceRequest::ReadState { path } => {
                            Ok(host::ResourceResponse::Bytes(library.data(
                                &program_path,
                                path,
                                16 << 20,
                            )?))
                        }
                        host::ResourceRequest::WriteState { .. } => anyhow::bail!(
                            "The offline UFS command has no private writable state directory"
                        ),
                        host::ResourceRequest::Browse { .. } => {
                            anyhow::bail!("The offline UFS command has no file browser")
                        }
                    }
                };
                resolve().map_err(mlua::Error::external)
            })
        };
        Ok(Self {
            samples,
            capability,
            revision,
        })
    }

    pub fn capability(&self) -> host::Resources {
        self.capability.clone()
    }

    /// Changes only when a newly resolved alias becomes available.
    pub fn revision(&self) -> u64 {
        self.revision.get()
    }

    /// Snapshot aliases while sharing the decoded PCM allocations.
    pub fn samples(&self) -> HashMap<String, Arc<Sample>> {
        self.samples.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bank_resource_callback_shares_pcm_and_publishes_only_successful_aliases() {
        let mut cursor = std::io::Cursor::new(Vec::new());
        {
            let mut wav = hound::WavWriter::new(
                &mut cursor,
                hound::WavSpec {
                    channels: 1,
                    sample_rate: 48000,
                    bits_per_sample: 16,
                    sample_format: hound::SampleFormat::Int,
                },
            )
            .unwrap();
            for value in [4096i16, -4096, 8192, 0] {
                wav.write_sample(value).unwrap();
            }
            wav.finalize().unwrap();
        }
        let wav = cursor.into_inner();
        let mut image = Vec::new();
        let mut encoder = png::Encoder::new(&mut image, 1, 4);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[255, 255, 255, 255, 255, 0, 255, 0, 0, 128, 128, 128])
            .unwrap();
        let mut bytes = vec![0u8; 320];
        bytes[..4].copy_from_slice(b"UFS2");
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        bytes[48..56].copy_from_slice(b"Authored");
        bytes.extend_from_slice(&wav);
        let image_offset = bytes.len() as u64;
        bytes.extend_from_slice(&image);
        let path =
            std::env::temp_dir().join(format!("kontra-bank-resource-{}.ufs", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let library = Rc::new(Library {
            bank: Ufs::open(&path).unwrap(),
            content_key: None,
            directory: Directory {
                files: vec![
                    Member {
                        record_offset: 320,
                        name: "authored.wav".into(),
                        path: Some("Samples/authored.wav".into()),
                        parent: None,
                        size: wav.len() as u64,
                        offset: 320,
                        mode: 0,
                        footer: Vec::new(),
                    },
                    Member {
                        record_offset: image_offset,
                        name: "disguised_128.wav".into(),
                        path: Some("Samples/disguised_128.wav".into()),
                        parent: None,
                        size: image.len() as u64,
                        offset: image_offset,
                        mode: 0,
                        footer: Vec::new(),
                    },
                ],
                directories: Vec::new(),
                records: Vec::new(),
                warnings: Vec::new(),
                metadata_key: 0,
            },
        });
        let resources =
            BankResources::new(library.clone(), "Programs/authored.uvip", HashMap::new()).unwrap();
        let read = resources.capability();
        let relative = "../Samples/authored.wav";
        let absolute = "/Samples/authored.wav";
        let request = |path: &str| host::ResourceRequest::ReadAudio {
            kind: host::ResourceKind::Sample,
            path: path.into(),
        };
        let image_path = "../Samples/disguised_128.wav";
        let program = program::parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><WaveTableOscillator WavetablePath="../Samples/disguised_128.wav"/><WaveTableOscillator WavetablePath="/Samples/authored.wav"/><SamplePlayer SamplePath="../Samples/authored.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let loaded = LoadedProgram {
            program,
            path: "Programs/authored.uvip".into(),
        };
        let initial = library.samples(&loaded).unwrap();
        assert!(initial[image_path].wavetable_image);
        assert_eq!(initial[image_path].wavetable_cycle_frames, Some(2048));
        assert!(!initial[relative].wavetable_image);
        assert!(Arc::ptr_eq(&initial[relative], &initial[absolute]));
        let image_resources = BankResources::new(library.clone(), &loaded.path, initial).unwrap();
        let image_read = image_resources.capability();
        for image_alias in [image_path, "/Samples/disguised_128.wav"] {
            assert!(
                image_read(&request(image_alias))
                    .unwrap_err()
                    .to_string()
                    .contains("Image wavetable")
            );
            assert_eq!(image_resources.revision(), 0);
            assert_eq!(image_resources.samples().len(), 3);
        }
        assert!(library.audio(&loaded.path, image_path).is_err());
        let invalid = LoadedProgram {
            program: program::parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="../Samples/disguised_128.wav"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap(),
            path: loaded.path,
        };
        assert!(library.samples(&invalid).is_err());
        assert_eq!(resources.revision(), 0);
        let host::ResourceResponse::Audio(info) = read(&request(relative)).unwrap() else {
            panic!()
        };
        assert_eq!((info.rate, info.channels, info.frames), (48000, 1, 4));
        assert_eq!(resources.revision(), 1);
        read(&request(relative)).unwrap();
        assert_eq!(resources.revision(), 1);
        read(&request(absolute)).unwrap();
        let samples = resources.samples();
        assert!(Arc::ptr_eq(&samples[relative], &samples[absolute]));
        assert_eq!(samples[relative].interleaved.value(0).unwrap(), 0.125);
        assert_eq!(resources.revision(), 2);
        assert!(read(&request("../../outside.wav")).is_err());
        assert_eq!(resources.revision(), 2);
        assert_eq!(resources.samples().len(), 2);
        assert!(
            read(&host::ResourceRequest::WriteState {
                path: "state".into(),
                bytes: vec![1]
            })
            .is_err()
        );
        // Exercise VM scheduling, a new resource alias and DSP state together.
        let program = program::parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="../Samples/authored.wav" BaseNote="60" Interpolation="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script><![CDATA[function onNote(e) postEvent(e);wait(0.01);changeVolume(e.id,0.5,false,true) end; function onController(e) local path=e.value==0 and '../../outside.wav' or '/Samples/authored.wav';local t=loadSample(Program.layers[1].keygroups[1].oscillators[1],path);assert(t.success) end]]></script></ScriptProcessor></EventProcessors></Program>"#).unwrap();
        let make_resources = || {
            BankResources::new(
                library.clone(),
                "Programs/authored.uvip",
                HashMap::from([(relative.into(), samples[relative].clone())]),
            )
            .unwrap()
        };
        let mut partitioned =
            super::super::player::Player::new(&program, BTreeMap::new(), make_resources(), 96000)
                .unwrap();
        let mut bulk =
            super::super::player::Player::new(&program, BTreeMap::new(), make_resources(), 96000)
                .unwrap();
        use super::super::script::{Input, InputKind};
        let note = |frame| Input {
            frame,
            kind: InputKind::NoteOn {
                channel: 0,
                note: 60,
                velocity: 100,
            },
        };
        let controller = |frame, value| Input {
            frame,
            kind: InputKind::Controller {
                channel: 0,
                controller: 1,
                value,
            },
        };
        let inputs = [
            note(0),
            controller(1, 127),
            note(1),
            Input {
                frame: 6,
                kind: InputKind::NoteOff {
                    channel: 0,
                    note: 60,
                },
            },
        ];
        let bad = [
            note(0),
            Input {
                frame: 1,
                kind: InputKind::Controller {
                    channel: 16,
                    controller: 1,
                    value: 127,
                },
            },
        ];
        assert!(partitioned.render(&bad, 2).is_err());
        assert_eq!(partitioned.current_frame(), 0);
        for tempo in [0.5, 1000.1, 1e100] {
            let transport = Input {
                frame: 1,
                kind: InputKind::Transport {
                    playing: true,
                    beat: 0.,
                    tempo,
                },
            };
            assert!(partitioned.render(&[note(0), transport], 2).is_err());
            assert_eq!(partitioned.current_frame(), 0);
        }
        let first = partitioned.render(&inputs[..1], 1).unwrap();
        assert_eq!(first.commands, 1);
        assert!(first.audio[0].iter().all(|sample| *sample > 0.));
        assert_eq!(partitioned.current_frame(), 1);
        assert!(partitioned.render(&[note(2)], 1).is_err());
        assert_eq!(partitioned.current_frame(), 1);
        let rest = partitioned.render(&inputs[1..], 7).unwrap();
        let full = bulk.render(&inputs, 8).unwrap();
        let combined = first
            .audio
            .into_iter()
            .chain(rest.audio)
            .collect::<Vec<_>>();
        assert_eq!(combined, full.audio);
        assert_eq!(first.commands + rest.commands, full.commands);
        assert_eq!(first.host_commands + rest.host_commands, full.host_commands);
        assert!(full.logs.is_empty());
        assert_eq!(partitioned.current_frame(), 8);
        assert!(partitioned.render(&[controller(8, 0)], 1).is_err());
        assert_eq!(partitioned.current_frame(), 8);
        assert!(
            partitioned
                .render(&[], 1)
                .unwrap_err()
                .to_string()
                .contains("must be replaced")
        );
        // Initialization loads aliases before the first Renderer cache snapshot.
        // The captured override paths must also prepare an equivalent fresh restore.
        let initialized = program::parse_program(r#"<Program><Inserts><Convolver Dry="1" Wet="0"/></Inserts><Layers><Layer><Keygroups><Keygroup><Oscillators><SamplePlayer SamplePath="../Samples/authored.wav" BaseNote="60" Interpolation="0"/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script><![CDATA[
          function onInit()
            assert(loadSample(Program.layers[1].keygroups[1].oscillators[1],'/Samples/authored.wav').success)
            assert(loadImpulse(Program.inserts[1],'/Samples/authored.wav').success)
          end
        ]]></script></ScriptProcessor></EventProcessors></Program>"#).unwrap();
        let mut initial = super::super::player::Player::new(
            &initialized,
            BTreeMap::new(),
            make_resources(),
            48000,
        )
        .unwrap();
        let saved = initial.saved_state().unwrap();
        let expected = initial.render(&[note(0)], 8).unwrap();
        assert!(expected.audio.iter().flatten().any(|value| *value != 0.));
        let mut restored = super::super::player::Player::new_with_state(
            &initialized,
            BTreeMap::new(),
            make_resources(),
            48000,
            None,
            Some(&saved),
        )
        .unwrap();
        assert_eq!(
            restored.render(&[note(0)], 8).unwrap().audio,
            expected.audio
        );

        // A global planned source must keep its clock through silent blocks.
        let planned = program::parse_program(r#"<Program><ControlSignalSources><StdRandom Name="Src" Rate="300" Depth=".7" Bipolar="1" TriggerMode="0"/></ControlSignalSources><Layers><Layer><Keygroups><Keygroup><Connections><SignalConnection Source="$Program/Src" Destination="Gain" Ratio="1"/></Connections><Oscillators><SamplePlayer SamplePath="../Samples/authored.wav" BaseNote="60"/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>"#).unwrap();
        let mut whole =
            super::super::player::Player::new(&planned, BTreeMap::new(), make_resources(), 48000)
                .unwrap();
        let mut split =
            super::super::player::Player::new(&planned, BTreeMap::new(), make_resources(), 48000)
                .unwrap();
        assert!(split.requires_planned_segments());
        assert!(split.render(&[], 255).is_err());
        assert_eq!(split.current_frame(), 0);
        let silent = split.render(&[], 256).unwrap();
        assert_eq!(silent.audio, vec![[0.; 2]; 256]);
        let inputs = [note(256), note(512)];
        let tail = split.render(&inputs, 512).unwrap();
        let full = whole.render(&inputs, 768).unwrap();
        assert_eq!(tail.audio, full.audio[256..]);
        assert!(tail.audio.iter().flatten().any(|value| *value != 0.));
        // A delayed note in padded lookahead changes envelope interpolation
        // before the file's end. Both processing modes must retain it.
        let padded = program::parse_program(r#"<Program><Layers><Layer><Keygroups><Keygroup Gain="1"><ControlSignalSources><AttackDecayEnv Name="Env" Attack="0" DecayTime=".2"/></ControlSignalSources><Connections><SignalConnection Source="$Keygroup/Env" Destination="Gain" Ratio="1"/></Connections><Oscillators><FmOscillator Gain=".25"/></Oscillators></Keygroup></Keygroups></Layer></Layers><EventProcessors><ScriptProcessor><script><![CDATA[function onNote(e) wait(1000-getTime());playNote(60,100,0);wait(3.125);playNote(72,100,0) end]]></script></ScriptProcessor></EventProcessors></Program>"#).unwrap();
        let mut bulk =
            super::super::player::Player::new(&padded, BTreeMap::new(), make_resources(), 48000)
                .unwrap();
        let mut split =
            super::super::player::Player::new(&padded, BTreeMap::new(), make_resources(), 48000)
                .unwrap();
        assert!(bulk.requires_planned_segments());
        let full = bulk.render(&[note(142)], 48384).unwrap();
        assert_eq!(full.commands, 2);
        let mut audio = Vec::new();
        for frame in (0..48384).step_by(256) {
            let block_inputs = if frame == 0 {
                vec![note(142)]
            } else {
                Vec::new()
            };
            audio.extend(split.render(&block_inputs, 256).unwrap().audio);
        }
        assert_eq!(audio, full.audio);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn resource_alias_budget_accounts_before_cache_growth() {
        let path = "Samples/authored.wav";
        let cost = alias_bytes(0, path).unwrap();
        assert_eq!(alias_bytes(ALIAS_LIMIT - cost, path).unwrap(), ALIAS_LIMIT);
        assert!(alias_bytes(ALIAS_LIMIT - cost + 1, path).is_err());
        assert!(alias_bytes(usize::MAX, path).is_err());
        assert!(alias_bytes(0, "").is_err());
        assert!(alias_bytes(0, &"x".repeat(4097)).is_err());
        assert!(alias_bytes(0, "bad\0name").is_err());
    }
    #[test]
    fn exact_resource_paths_and_ambiguous_names() {
        let member = |path: &str| Member {
            record_offset: 1,
            name: path.rsplit('/').next().unwrap().into(),
            path: Some(path.into()),
            parent: None,
            size: 0,
            offset: 0,
            mode: 0,
            footer: Vec::new(),
        };
        let directory = Directory {
            files: vec![
                member("Samples/a.wav"),
                member("Other/a.wav"),
                member("Scripts/data.lua"),
            ],
            directories: Vec::new(),
            records: Vec::new(),
            warnings: Vec::new(),
            metadata_key: 0,
        };
        assert_eq!(
            resource(&directory, "Programs/p.uvip", ".\\..\\Samples\\a.wav")
                .unwrap()
                .path
                .as_deref(),
            Some("Samples/a.wav")
        );
        assert!(resolve_member(&directory, "a.wav").is_err());
        assert!(resolve_member(&directory, "Missing/a.wav").is_err());
        assert!(resource(&directory, "Programs/p.uvip", "../../a.wav").is_err());
        assert!(resolve_member(&directory, "$Other/a.wav").is_err());
        let bundle = resources(&directory, "Programs/p.uvip", "../Samples/*a.wav*a.wav").unwrap();
        assert_eq!(bundle.len(), 2);
        assert!(resources(&directory, "Programs/p.uvip", "../Samples/*a.wav*").is_err());
        assert!(resources(&directory, "Programs/p.uvip", "../Samples/*../Other/a.wav").is_err());
        assert_eq!(
            resolve_member(&directory, "data.lua").unwrap().name,
            "data.lua"
        );
        let path = std::env::temp_dir().join(format!("uvi-identity-{}.ufs", std::process::id()));
        let mut header = [0u8; 320];
        header[..4].copy_from_slice(b"UFS2");
        header[4..8].copy_from_slice(&3u32.to_le_bytes());
        std::fs::write(&path, header).unwrap();
        let library = Library {
            bank: Ufs::open(&path).unwrap(),
            directory,
            content_key: None,
        };
        assert_eq!(
            library
                .audio_identity("Programs/p.uvip", "../Samples/a.wav")
                .unwrap(),
            library
                .audio_identity("Programs/p.uvip", "/Samples/./a.wav")
                .unwrap()
        );
        assert_ne!(
            library
                .audio_identity("Programs/p.uvip", "../Samples/a.wav")
                .unwrap(),
            library
                .audio_identity("Programs/p.uvip", "../Samples/*a.wav*a.wav")
                .unwrap()
        );
        std::fs::remove_file(path).unwrap();
    }
}
