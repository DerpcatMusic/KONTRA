//! Explicit local NKA files. Only `perform` does I/O; owned buffers return to control.
use crate::{Error, ScriptInstanceId, Text};
use std::io::{BufRead, BufReader, BufWriter, Write};

pub const ARRAY_FILE_SERVICE: u16 = u16::MAX - 1;
pub const ARRAY_FILE_JOBS: usize = 8;
// A concurrency target, never an array-size ceiling: one large payload still fits.
const SNAPSHOT_TARGET_BYTES: usize = 8 * 1024 * 1024;

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
        if self.len == 0 {
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
    /// Only writes lease request-time snapshots; reads own metadata alone.
    snapshot: Option<usize>,
}
#[derive(Debug)]
struct ArrayFileSnapshot {
    leased: bool,
    payload: ArrayFilePayload,
}
#[derive(Debug, Default)]
pub(crate) struct ArrayFileState {
    pub jobs: Vec<ArrayFileJob>,
    snapshots: Vec<ArrayFileSnapshot>,
}
impl ArrayFileState {
    fn snapshot_jobs(numbers: usize, texts: usize) -> Result<usize, Error> {
        let bytes = numbers
            .checked_mul(std::mem::size_of::<i64>())
            .and_then(|numbers| {
                texts
                    .checked_mul(std::mem::size_of::<Text>())
                    .and_then(|texts| numbers.checked_add(texts))
            })
            .ok_or(Error::Capacity)?;
        // Share the memory target across jobs; arrays larger than it use one slot.
        Ok(if bytes == 0 {
            0
        } else {
            ARRAY_FILE_JOBS.min((SNAPSHOT_TARGET_BYTES / bytes).max(1))
        })
    }
    pub fn dimensions(arrays: &[ArrayFileArray]) -> Result<(usize, usize, usize), Error> {
        let (mut numbers, mut texts) = (0, 0);
        for array in arrays {
            array.validate()?;
            if array.kind == ArrayFileKind::Text {
                texts = texts.max(array.len as usize);
            } else {
                numbers = numbers.max(array.len as usize);
            }
        }
        Ok((numbers, texts, Self::snapshot_jobs(numbers, texts)?))
    }
    pub fn prepared(arrays: &[ArrayFileArray]) -> Self {
        let (numbers, texts, snapshots) = Self::dimensions(arrays).unwrap_or_default();
        Self {
            jobs: arrays.first().map_or_else(Vec::new, |array| {
                (0..ARRAY_FILE_JOBS)
                    .map(|_| ArrayFileJob {
                        id: 0,
                        array: *array,
                        path: Text::default(),
                        write: false,
                        snapshot: None,
                    })
                    .collect()
            }),
            snapshots: (0..snapshots)
                .map(|_| ArrayFileSnapshot {
                    leased: false,
                    payload: ArrayFilePayload::prepared(numbers, texts),
                })
                .collect(),
        }
    }
    fn release_snapshot(&mut self, job: usize) {
        if let Some(index) = self.jobs[job].snapshot.take() {
            let snapshot = &mut self.snapshots[index];
            debug_assert!(snapshot.leased);
            snapshot.payload.clear();
            snapshot.leased = false;
        }
    }
    pub fn cancel(&mut self) -> usize {
        let mut count = 0;
        for job in &mut self.jobs {
            count += usize::from(job.id != 0);
            job.id = 0;
            job.snapshot = None;
        }
        for snapshot in &mut self.snapshots {
            snapshot.payload.clear();
            snapshot.leased = false;
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_nka_takes_its_member_identity_and_a_short_prefix() {
        // Conflux: a `%num_IRs` header under the member `gui__fx__num_irs`, and
        // `!gui__fx__IR_names[128]` with only 2 names in the file.
        let count = ArrayFileArray {
            key: 0,
            name: Text::new("%gui__fx__num_IRs"),
            kind: ArrayFileKind::Integer,
            offset: 0,
            len: 1,
        };
        let done = ArrayFileCompletion::from_bytes(count, b"%num_IRs\n8\n").unwrap();
        assert_eq!(done.numbers(), &[8]);
        let names = ArrayFileArray {
            name: Text::new("!gui__fx__IR_names"),
            kind: ArrayFileKind::Text,
            len: 128,
            ..count
        };
        let done = ArrayFileCompletion::from_bytes(names, b"!gui__fx__IR_names\nChamber\nHall\n").unwrap();
        assert_eq!(done.texts().len(), 2);
        // Explicit path reads keep the header name and exact length checks.
        assert!(decode(count, &b"%other\n1\n"[..], false).is_err());
        assert!(decode(count, &b"%num_IRs\n1\n"[..], false).is_err());
    }

    #[test]
    fn snapshot_jobs_scale_concurrency_without_rejecting_large_arrays() {
        // Large payloads get one reserved slot; only arithmetic overflow fails.
        let numbers = SNAPSHOT_TARGET_BYTES / std::mem::size_of::<i64>();
        assert_eq!(ArrayFileState::snapshot_jobs(numbers, 0), Ok(1));
        assert_eq!(ArrayFileState::snapshot_jobs(numbers + 1, 0), Ok(1));
        assert_eq!(ArrayFileState::snapshot_jobs(numbers, 1), Ok(1));
        assert_eq!(
            ArrayFileState::snapshot_jobs(usize::MAX, 0),
            Err(Error::Capacity)
        );
        assert_eq!(
            ArrayFileState::snapshot_jobs(0, usize::MAX),
            Err(Error::Capacity)
        );
        let texts = usize::MAX / std::mem::size_of::<Text>();
        assert_eq!(
            ArrayFileState::snapshot_jobs(numbers, texts),
            Err(Error::Capacity)
        );
        assert_eq!(ArrayFileState::snapshot_jobs(1, 1), Ok(ARRAY_FILE_JOBS));
        assert_eq!(ArrayFileState::dimensions(&[]), Ok((0, 0, 0)));
        assert!(ArrayFileState::prepared(&[]).jobs.is_empty());
    }

    #[test]
    fn snapshot_pool_prepares_only_the_derived_slots_within_payload_budget() {
        let arrays = [
            ArrayFileArray {
                key: 0,
                name: Text::new("%data"),
                kind: ArrayFileKind::Integer,
                offset: 0,
                len: 16000,
            },
            ArrayFileArray {
                key: 1,
                name: Text::new("!names"),
                kind: ArrayFileKind::Text,
                offset: 0,
                len: 4000,
            },
        ];
        let bytes = 16000 * std::mem::size_of::<i64>() + 4000 * std::mem::size_of::<Text>();
        let slots = ARRAY_FILE_JOBS.min((SNAPSHOT_TARGET_BYTES / bytes).max(1));
        assert_eq!(
            ArrayFileState::dimensions(&arrays),
            Ok((16000, 4000, slots))
        );
        let prepared = ArrayFileState::prepared(&arrays);
        assert_eq!(prepared.jobs.len(), ARRAY_FILE_JOBS);
        assert!(
            prepared
                .jobs
                .iter()
                .all(|j| j.id == 0 && j.snapshot.is_none())
        );
        assert_eq!(prepared.snapshots.len(), slots);
        let reserved: usize = prepared
            .snapshots
            .iter()
            .map(|snapshot| {
                assert!(!snapshot.leased);
                let payload = &snapshot.payload;
                assert!(payload.numbers.is_empty());
                assert!(payload.texts.is_empty());
                assert_eq!(payload.numbers.capacity(), 16000);
                assert_eq!(payload.texts.capacity(), 4000);
                payload.numbers.capacity() * std::mem::size_of::<i64>()
                    + payload.texts.capacity() * std::mem::size_of::<Text>()
            })
            .sum();
        assert_eq!(reserved, slots * bytes);
        assert!(reserved <= SNAPSHOT_TARGET_BYTES);
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
        let len = u32::try_from(effect.args[2]).map_err(|_| Error::InvalidInput)?;
        if len == 0 {
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
                len,
            },
            path: Text::default(),
            write: false,
            captured: false,
            payload: ArrayFilePayload::prepared(
                if kind != ArrayFileKind::Text {
                    len as usize
                } else {
                    0
                },
                if kind == ArrayFileKind::Text {
                    len as usize
                } else {
                    0
                },
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
    /// A completed read of NKA bytes the loader already holds (a resource
    /// container's data folder); no path, no I/O.
    pub fn from_bytes(array: ArrayFileArray, bytes: &[u8]) -> Result<Self, Error> {
        array.validate()?;
        Ok(Self {
            job: 0,
            instance: ScriptInstanceId(0),
            success: true,
            array,
            path: Text::default(),
            write: false,
            captured: true,
            payload: decode(array, bytes, true)?,
        })
    }
    pub fn numbers(&self) -> &[i64] {
        &self.payload.numbers
    }
    pub fn texts(&self) -> &[Text] {
        &self.payload.texts
    }
    /// As [`Self::perform`] for a host that may only read (library data): a
    /// save fails without touching disk, as at a read-only location.
    pub fn perform_read_only(&mut self) -> Result<(), Error> {
        if self.write {
            self.success = false;
            return Err(Error::InvalidInput);
        }
        self.perform()
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
            atomic_write(self.path.as_str(), self.array, &self.payload)?;
        } else {
            let file = std::fs::File::open(self.path.as_str()).map_err(|_| Error::InvalidInput)?;
            if !file.metadata().map_err(|_| Error::InvalidInput)?.is_file() {
                return Err(Error::InvalidInput);
            }
            self.payload = decode(self.array, BufReader::new(file), false)?;
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
/// `library` reads a Kontakt data-folder member (`Resources/data/*.nka`), which
/// the container already identifies: its header name is informational (Conflux
/// writes `%num_IRs` for `%gui__fx__num_IRs`), and a short file leaves the
/// rest of the declared array as it was, as Kontakt does.
fn decode(array: ArrayFileArray, mut reader: impl BufRead, library: bool) -> Result<ArrayFilePayload, Error> {
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|_| Error::InvalidInput)?;
    if !library && line.lines().next() != Some(array.name.as_str()) {
        return Err(Error::InvalidInput);
    }
    let mut payload = ArrayFilePayload::prepared(
        if array.kind == ArrayFileKind::Text {
            0
        } else {
            array.len as usize
        },
        if array.kind == ArrayFileKind::Text {
            array.len as usize
        } else {
            0
        },
    );
    for _ in 0..array.len {
        line.clear();
        if reader
            .read_line(&mut line)
            .map_err(|_| Error::InvalidInput)?
            == 0
        {
            if library {
                break;
            }
            return Err(Error::InvalidInput);
        }
        let value = line.lines().next().unwrap_or("");
        match array.kind {
            ArrayFileKind::Integer => payload.numbers.push(i64::from(
                value
                    .trim()
                    .parse::<i32>()
                    .map_err(|_| Error::InvalidInput)?,
            )),
            ArrayFileKind::Real => {
                let value = value
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| Error::InvalidInput)?;
                if !value.is_finite() {
                    return Err(Error::InvalidInput);
                }
                payload.numbers.push(crate::real_bits(value));
            }
            ArrayFileKind::Text => payload.texts.push(Text::try_new(value)?),
        }
    }
    // Values past the declared length are ignored, as Kontakt does: a library
    // shares one data file between instruments that declare fewer entries.
    let read = ArrayFileArray {
        len: (payload.numbers.len() + payload.texts.len()) as u32,
        ..array
    };
    if !payload.valid(if library { read } else { array }) {
        return Err(Error::InvalidInput);
    }
    Ok(payload)
}
// Port of our frozen v1 sibling-temporary replacement; no lossy/case-fallback reader.
fn atomic_write(
    path: &str,
    array: ArrayFileArray,
    payload: &ArrayFilePayload,
) -> Result<(), Error> {
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
        {
            let mut writer = BufWriter::new(&mut file);
            writeln!(writer, "{}", array.name.as_str())?;
            for value in &payload.numbers {
                if array.kind == ArrayFileKind::Real {
                    writeln!(writer, "{}", crate::real(*value))?;
                } else {
                    writeln!(writer, "{value}")?;
                }
            }
            for value in &payload.texts {
                writeln!(writer, "{}", value.as_str())?;
            }
            writer.flush()?;
        }
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
        let job_index = bank
            .array_files
            .jobs
            .iter()
            .position(|j| j.id == 0)
            .ok_or(Error::Capacity)?;
        let snapshot_index = if write {
            let index = bank
                .array_files
                .snapshots
                .iter()
                .position(|s| !s.leased)
                .ok_or(Error::Capacity)?;
            let payload = &bank.array_files.snapshots[index].payload;
            if (if array.kind == ArrayFileKind::Text {
                payload.texts.capacity()
            } else {
                payload.numbers.capacity()
            }) < array.len as usize
            {
                return Err(Error::Capacity);
            }
            Some(index)
        } else {
            None
        };
        let next = generation
            .midi_object
            .next_job
            .checked_add(1)
            .ok_or(Error::Capacity)?;
        let callbacks = generation.callbacks.checked_add(1).ok_or(Error::Capacity)?;
        if let Some(index) = snapshot_index {
            let snapshot = &mut bank.array_files.snapshots[index];
            snapshot.payload.clear();
            let range = array.offset as usize..(array.offset + array.len) as usize;
            if array.kind == ArrayFileKind::Text {
                snapshot.payload.texts.extend_from_slice(&bank.texts[range]);
            } else {
                snapshot
                    .payload
                    .numbers
                    .extend_from_slice(&bank.cells[range]);
            }
            snapshot.leased = true;
        }
        let job = &mut bank.array_files.jobs[job_index];
        job.array = array;
        job.path = path;
        job.write = write;
        job.snapshot = snapshot_index;
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
        if job.write {
            let index = job.snapshot.ok_or(Error::InvalidInput)?;
            let snapshot = &generation.scripts[output.instance.0 as usize]
                .array_files
                .snapshots[index];
            if !snapshot.leased {
                return Err(Error::InvalidInput);
            }
            output.payload.copy(&snapshot.payload)?;
        } else {
            output.payload.clear();
        }
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
        bank.array_files.release_snapshot(index);
        bank.array_files.jobs[index].id = 0;
        generation.callbacks -= 1;
        self.finish_async_callback(plan, output.job, output.success, callback)
    }
}
