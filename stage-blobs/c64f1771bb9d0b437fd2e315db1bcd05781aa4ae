//! Explicit local NKA files. Only `perform` does I/O; owned buffers return to control.
use crate::{Error, ScriptInstanceId, Text};
use std::io::{Read, Write};

pub const ARRAY_FILE_SERVICE: u16 = u16::MAX - 1;
pub const ARRAY_FILE_JOBS: usize = 8;
pub const ARRAY_FILE_MAX_CELLS: usize = 16384;
pub const ARRAY_FILE_MAX_BYTES: usize = 2 * 1024 * 1024;
const SNAPSHOT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayFileKind {
    Integer,
    Real,
    Text,
}

/// Exact source identity and typed range in one script bank, never a UI ID.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArrayFileArray {
    pub key: u32,
    pub name: Text,
    pub kind: ArrayFileKind,
    pub offset: u32,
    pub len: u32,
}
impl ArrayFileArray {
    pub fn validate(&self) -> Result<(), Error> {
        let sigil = match self.kind {
            ArrayFileKind::Integer => '%',
            ArrayFileKind::Real => '?',
            ArrayFileKind::Text => '!',
        };
        if self.len == 0 || self.len as usize > ARRAY_FILE_MAX_CELLS {
            return Err(Error::Capacity);
        }
        if self.name.is_truncated()
            || !self.name.as_str().starts_with(sigil)
            || self.name.as_str().len() < 2
            || self.name.as_str().contains(['\n', '\r', '\0'])
        {
            return Err(Error::InvalidInput);
        }
        self.offset.checked_add(self.len).ok_or(Error::Capacity)?;
        Ok(())
    }
}

#[derive(Debug, Default)]
pub(crate) struct ArrayFilePayload {
    pub numbers: Vec<i64>,
    pub texts: Vec<Text>,
}
impl ArrayFilePayload {
    fn prepared(numbers: usize, texts: usize) -> Self {
        Self {
            numbers: Vec::with_capacity(numbers),
            texts: Vec::with_capacity(texts),
        }
    }
    fn clear(&mut self) {
        self.numbers.clear();
        self.texts.clear();
    }
    fn copy(&mut self, other: &Self) -> Result<(), Error> {
        if self.numbers.capacity() < other.numbers.len()
            || self.texts.capacity() < other.texts.len()
        {
            return Err(Error::Capacity);
        }
        self.clear();
        self.numbers.extend_from_slice(&other.numbers);
        self.texts.extend_from_slice(&other.texts);
        Ok(())
    }
    fn valid(&self, array: ArrayFileArray) -> bool {
        match array.kind {
            ArrayFileKind::Integer => {
                self.texts.is_empty()
                    && self.numbers.len() == array.len as usize
                    && self.numbers.iter().all(|v| i32::try_from(*v).is_ok())
            }
            ArrayFileKind::Real => {
                self.texts.is_empty()
                    && self.numbers.len() == array.len as usize
                    && self.numbers.iter().all(|v| crate::real(*v).is_finite())
            }
            ArrayFileKind::Text => {
                self.numbers.is_empty()
                    && self.texts.len() == array.len as usize
                    && self
                        .texts
                        .iter()
                        .all(|v| !v.is_truncated() && !v.as_str().contains(['\r', '\n', '\0']))
            }
        }
    }
}

#[derive(Debug)]
pub(crate) struct ArrayFileJob {
    pub id: i32,
    pub array: ArrayFileArray,
    pub path: Text,
    pub write: bool,
    pub payload: ArrayFilePayload,
}
#[derive(Debug, Default)]
pub(crate) struct ArrayFileState {
    pub jobs: Vec<ArrayFileJob>,
}
impl ArrayFileState {
    pub fn dimensions(arrays: &[ArrayFileArray]) -> Result<(usize, usize), Error> {
        let (mut numbers, mut texts) = (0, 0);
        for array in arrays {
            array.validate()?;
            if array.kind == ArrayFileKind::Text {
                texts = texts.max(array.len as usize);
            } else {
                numbers = numbers.max(array.len as usize);
            }
        }
        let bytes = numbers * std::mem::size_of::<i64>() + texts * std::mem::size_of::<Text>();
        if bytes * ARRAY_FILE_JOBS > SNAPSHOT_BYTES {
            return Err(Error::Capacity);
        }
        Ok((numbers, texts))
    }
    pub fn prepared(arrays: &[ArrayFileArray]) -> Self {
        let (numbers, texts) = Self::dimensions(arrays).unwrap_or_default();
        Self {
            jobs: arrays.first().map_or_else(Vec::new, |array| {
                (0..ARRAY_FILE_JOBS)
                    .map(|_| ArrayFileJob {
                        id: 0,
                        array: *array,
                        path: Text::default(),
                        write: false,
                        payload: ArrayFilePayload::prepared(numbers, texts),
                    })
                    .collect()
            }),
        }
    }
    pub fn cancel(&mut self) -> usize {
        let mut count = 0;
        for job in &mut self.jobs {
            count += usize::from(job.id != 0);
            job.id = 0;
            job.payload.clear();
        }
        count
    }
}

/// Control-owned capture/completion. Never drop on audio: the control reply owns it.
#[derive(Debug)]
pub struct ArrayFileCompletion {
    pub job: i32,
    pub instance: ScriptInstanceId,
    pub success: bool,
    pub(crate) array: ArrayFileArray,
    pub(crate) path: Text,
    pub(crate) write: bool,
    pub(crate) captured: bool,
    pub(crate) payload: ArrayFilePayload,
}
impl ArrayFileCompletion {
    /// Allocate off audio using the bounded, runtime-authored request metadata.
    pub fn from_effect(effect: &crate::Effect) -> Result<Self, Error> {
        if effect.service != ARRAY_FILE_SERVICE || effect.count != 4 {
            return Err(Error::InvalidInput);
        }
        let len = usize::try_from(effect.args[2]).map_err(|_| Error::InvalidInput)?;
        if len == 0 || len > ARRAY_FILE_MAX_CELLS {
            return Err(Error::Capacity);
        }
        let kind = match effect.args[3] {
            0 => ArrayFileKind::Integer,
            1 => ArrayFileKind::Real,
            2 => ArrayFileKind::Text,
            _ => return Err(Error::InvalidInput),
        };
        let job = i32::try_from(effect.args[0]).map_err(|_| Error::InvalidInput)?;
        if job <= 0 {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            job,
            instance: effect.instance.ok_or(Error::InvalidInput)?,
            success: false,
            array: ArrayFileArray {
                key: u32::try_from(effect.args[1]).map_err(|_| Error::InvalidInput)?,
                name: Text::default(),
                kind,
                offset: 0,
                len: len as u32,
            },
            path: Text::default(),
            write: false,
            captured: false,
            payload: ArrayFilePayload::prepared(
                if kind != ArrayFileKind::Text { len } else { 0 },
                if kind == ArrayFileKind::Text { len } else { 0 },
            ),
        })
    }
    /// Preparation-only synchronous entry; typed values must be complete.
    pub fn synchronous(
        array: ArrayFileArray,
        path: &str,
        write: bool,
        numbers: Vec<i64>,
        texts: Vec<Text>,
    ) -> Result<Self, Error> {
        array.validate()?;
        let path = Text::try_new(path)?;
        validate_path(path.as_str())?;
        Ok(Self {
            job: 0,
            instance: ScriptInstanceId(0),
            success: false,
            array,
            path,
            write,
            captured: true,
            payload: ArrayFilePayload { numbers, texts },
        })
    }
    pub fn numbers(&self) -> &[i64] {
        &self.payload.numbers
    }
    pub fn texts(&self) -> &[Text] {
        &self.payload.texts
    }
    /// Worker/preparation thread only. Failures leave the destination unchanged.
    pub fn perform(&mut self) -> Result<(), Error> {
        self.success = false;
        if !self.captured {
            return Err(Error::InvalidInput);
        }
        self.array.validate()?;
        if self.path.is_truncated() {
            return Err(Error::Capacity);
        }
        validate_path(self.path.as_str())?;
        if self.write {
            if !self.payload.valid(self.array) {
                return Err(Error::InvalidInput);
            }
            let bytes = encode(self.array, &self.payload)?;
            atomic_write(self.path.as_str(), &bytes)?;
        } else {
            let file = std::fs::File::open(self.path.as_str()).map_err(|_| Error::InvalidInput)?;
            if !file.metadata().map_err(|_| Error::InvalidInput)?.is_file() {
                return Err(Error::InvalidInput);
            }
            let mut bytes = Vec::new();
            file.take(ARRAY_FILE_MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::InvalidInput)?;
            if bytes.len() > ARRAY_FILE_MAX_BYTES {
                return Err(Error::Capacity);
            }
            self.payload = decode(self.array, &bytes)?;
        }
        self.success = true;
        Ok(())
    }
}
fn validate_path(path: &str) -> Result<(), Error> {
    if path.is_empty() || path.contains(['\\', '\0']) || !std::path::Path::new(path).is_absolute() {
        Err(Error::InvalidInput)
    } else {
        Ok(())
    }
}
fn decode(array: ArrayFileArray, bytes: &[u8]) -> Result<ArrayFilePayload, Error> {
    let text = std::str::from_utf8(bytes).map_err(|_| Error::InvalidInput)?;
    let mut lines = text.lines();
    if lines.next() != Some(array.name.as_str()) {
        return Err(Error::InvalidInput);
    }
    let mut payload = ArrayFilePayload::default();
    for (index, line) in lines.enumerate() {
        if index >= array.len as usize {
            return Err(Error::InvalidInput);
        }
        match array.kind {
            ArrayFileKind::Integer => payload.numbers.push(i64::from(
                line.trim()
                    .parse::<i32>()
                    .map_err(|_| Error::InvalidInput)?,
            )),
            ArrayFileKind::Real => {
                let value = line
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| Error::InvalidInput)?;
                if !value.is_finite() {
                    return Err(Error::InvalidInput);
                }
                payload.numbers.push(crate::real_bits(value));
            }
            ArrayFileKind::Text => payload.texts.push(Text::try_new(line)?),
        }
    }
    if !payload.valid(array) {
        return Err(Error::InvalidInput);
    }
    Ok(payload)
}
fn encode(array: ArrayFileArray, payload: &ArrayFilePayload) -> Result<Vec<u8>, Error> {
    use std::fmt::Write as _;
    let mut text = format!("{}\n", array.name.as_str());
    for value in &payload.numbers {
        if array.kind == ArrayFileKind::Real {
            writeln!(text, "{}", crate::real(*value)).map_err(|_| Error::InvalidInput)?;
        } else {
            writeln!(text, "{value}").map_err(|_| Error::InvalidInput)?;
        }
        if text.len() > ARRAY_FILE_MAX_BYTES {
            return Err(Error::Capacity);
        }
    }
    for value in &payload.texts {
        text.push_str(value.as_str());
        text.push('\n');
        if text.len() > ARRAY_FILE_MAX_BYTES {
            return Err(Error::Capacity);
        }
    }
    Ok(text.into_bytes())
}
// Port of our frozen v1 sibling-temporary replacement; no lossy/case-fallback reader.
fn atomic_write(path: &str, bytes: &[u8]) -> Result<(), Error> {
    use std::{
        path::Path,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = Path::new(path);
    let parent = path.parent().ok_or(Error::InvalidInput)?;
    let filename = path.file_name().ok_or(Error::InvalidInput)?;
    let permissions = match std::fs::metadata(path) {
        Ok(meta) if !meta.is_file() || meta.permissions().readonly() => {
            return Err(Error::InvalidInput);
        }
        Ok(meta) => Some(meta.permissions()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err(Error::InvalidInput),
    };
    let temporary = parent.join(format!(
        ".{}.kontra-{}-{}.tmp",
        filename.to_string_lossy(),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| Error::InvalidInput)?;
    let result = (|| {
        file.write_all(bytes)?;
        file.flush()?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result.map_err(|_| Error::InvalidInput)
}

impl crate::Runtime {
    pub(super) fn request_array_file(
        &mut self,
        id: crate::BehaviorId,
        plan: crate::PlanId,
        key: u32,
        path: Text,
        write: bool,
    ) -> Result<i32, Error> {
        if self.ops.effects.len() == crate::ops::EFFECT_CAPACITY {
            return Err(Error::Capacity);
        }
        let c = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        let instance = generation.prepared.programs[c.program]
            .script_instance
            .ok_or(Error::InvalidInput)?;
        let array = *generation.prepared.script_initial[instance.0 as usize]
            .array_files
            .iter()
            .find(|a| a.key == key)
            .ok_or(Error::InvalidInput)?;
        let bank = &mut generation.scripts[instance.0 as usize];
        let job = bank
            .array_files
            .jobs
            .iter_mut()
            .find(|j| j.id == 0)
            .ok_or(Error::Capacity)?;
        let next = generation
            .midi_object
            .next_job
            .checked_add(1)
            .ok_or(Error::Capacity)?;
        let callbacks = generation.callbacks.checked_add(1).ok_or(Error::Capacity)?;
        if write
            && (if array.kind == ArrayFileKind::Text {
                job.payload.texts.capacity()
            } else {
                job.payload.numbers.capacity()
            }) < array.len as usize
        {
            return Err(Error::Capacity);
        }
        job.payload.clear();
        if write {
            let range = array.offset as usize..(array.offset + array.len) as usize;
            if array.kind == ArrayFileKind::Text {
                job.payload.texts.extend_from_slice(&bank.texts[range]);
            } else {
                job.payload.numbers.extend_from_slice(&bank.cells[range]);
            }
        }
        job.array = array;
        job.path = path;
        job.write = write;
        job.id = next;
        generation.midi_object.next_job = next;
        generation.callbacks = callbacks;
        self.ops.effects.push_back(crate::Effect {
            plan,
            instance: Some(instance),
            service: ARRAY_FILE_SERVICE,
            args: [
                i64::from(next),
                i64::from(key),
                i64::from(array.len),
                array.kind as i64,
                0,
                0,
            ],
            count: 4,
            text: None,
        });
        Ok(next)
    }
    pub fn capture_array_file(
        &self,
        plan: crate::PlanId,
        output: &mut ArrayFileCompletion,
    ) -> Result<(), Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let job = generation
            .scripts
            .get(output.instance.0 as usize)
            .ok_or(Error::InvalidInput)?
            .array_files
            .jobs
            .iter()
            .find(|j| j.id == output.job && j.id != 0)
            .ok_or(Error::StaleHandle)?;
        if output.array.key != job.array.key
            || output.array.len != job.array.len
            || output.array.kind != job.array.kind
        {
            return Err(Error::InvalidInput);
        }
        output.payload.copy(&job.payload)?;
        output.array = job.array;
        output.path = job.path;
        output.write = job.write;
        output.captured = true;
        Ok(())
    }
    pub fn complete_array_file(
        &mut self,
        plan: crate::PlanId,
        output: &mut ArrayFileCompletion,
    ) -> Result<(), Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let bank = generation
            .scripts
            .get(output.instance.0 as usize)
            .ok_or(Error::InvalidInput)?;
        let index = bank
            .array_files
            .jobs
            .iter()
            .position(|j| j.id == output.job && j.id != 0)
            .ok_or(Error::StaleHandle)?;
        let job = &bank.array_files.jobs[index];
        if !output.captured
            || output.array != job.array
            || output.write != job.write
            || output.path != job.path
        {
            return Err(Error::InvalidInput);
        }
        let callback = self.preflight_async_callback(plan, output.instance)?;
        let generation = self.plans.get_mut(plan.0).unwrap();
        let bank = &mut generation.scripts[output.instance.0 as usize];
        if output.success && !output.write {
            output.success = output.payload.valid(output.array);
            if output.success {
                let range =
                    output.array.offset as usize..(output.array.offset + output.array.len) as usize;
                if output.array.kind == ArrayFileKind::Text {
                    bank.texts[range].copy_from_slice(&output.payload.texts);
                } else {
                    for (cell, value) in range.zip(&output.payload.numbers) {
                        let changed = bank.cells[cell] != *value;
                        bank.cells[cell] = *value;
                        bank.mark_captured_cell(cell, changed);
                    }
                }
                generation.script_revision = generation.script_revision.wrapping_add(1);
            }
        }
        bank.array_files.jobs[index].id = 0;
        bank.array_files.jobs[index].payload.clear();
        generation.callbacks -= 1;
        self.finish_async_callback(plan, output.job, output.success, callback)
    }
}
