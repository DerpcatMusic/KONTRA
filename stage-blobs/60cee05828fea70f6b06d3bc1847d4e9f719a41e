//! Off-thread NKA files. Requests, paths and save snapshots are prepared before audio.
use super::{compile::{Ty, Var, VarId}, Value};

pub(super) const ARRAY_QUEUE: usize = 8;
const PATH_BYTES: usize = 4096;

/// An owned file job. Perform/recycle it off the audio thread; installing its
/// result retains the job for worker disposal, never dropping strings on audio.
#[derive(Debug)]
pub struct ArrayJob {
    pub slot: u8,
    pub id: i32,
    pub(super) var: VarId,
    pub(super) path: String,
    name: Box<str>,
    ty: Ty,
    len: usize,
    pub(super) values: Option<Value>,
    pub(super) write: bool,
    pub(super) snapshot: Option<Value>,
    pub(super) progress: usize,
    pub(super) validated: bool,
    pub(super) success: bool,
    pub(super) failure: Option<&'static str>,
    detail: Option<String>,
}

impl ArrayJob {
    pub(super) fn prepared(slot: u8, var: VarId, info: &Var) -> Self {
        Self { slot, id: -1, var, path: String::with_capacity(PATH_BYTES),
            name: info.name.trim_start_matches(['%', '!', '?', '$', '@', '~']).into(),
            ty: info.ty, len: info.len.unwrap_or(0) as usize, values: None,
            write: false, snapshot: None, progress: 0, validated: false, success: false, failure: None, detail: None }
    }
    pub fn path(&self) -> &str { &self.path }
    pub fn is_write(&self) -> bool { self.write }
    /// File I/O and parse buffers belong only to this non-audio operation.
    pub fn perform(&mut self) -> bool {
        self.values = None;
        self.failure = None;
        self.detail = None;
        if self.write {
            match self.snapshot.as_ref().ok_or_else(|| "save_array_str: snapshot was not prepared".to_owned())
                .and_then(|value| save_nka(&self.path, self.ty, &self.name, value))
            {
                Ok(()) => self.success = true,
                Err(error) => {
                    self.success = false;
                    self.failure = Some("save_array_str: file could not be written; see diagnostics log");
                    self.detail = Some(error);
                }
            }
            self.validated = true;
            return self.success;
        }
        match read_file_path(&self.path) {
            Err(error) => {
                self.failure = Some("load_array_str: file could not be read");
                self.detail = Some(error);
            }
            Ok(bytes) => match parse_nka(&bytes, self.ty, &self.name) {
                Ok(Value::Array(mut values)) => {
                    values.truncate(self.len);
                    self.values = Some(Value::Array(values));
                }
                Err(error) => self.failure = Some(error),
                _ => unreachable!(),
            },
        }
        self.validated = self.ty != Ty::Str;
        self.success = self.values.is_some();
        self.success
    }
    pub fn error(&self) -> Option<&str> { self.detail.as_deref().or(self.failure) }
    /// Called by the worker after the audio thread has installed the result.
    pub fn recycle(&mut self) {
        self.values = None; self.detail = None; self.failure = None;
        self.path.clear(); self.write = false; self.id = -1; self.progress = 0; self.validated = false; self.success = false;
    }
}

/// A file a script names, matching names without case when the exact
/// path is not there (libraries are made on case-insensitive systems).
pub(super) fn read_path(path: &str) -> Option<Vec<u8>> { read_file_path(path).ok() }

fn read_file_path(path: &str) -> Result<Vec<u8>, String> {
    use std::path::{Component, Path, PathBuf};
    let path = Path::new(path);
    match crate::resources::read_file(path) {
        Ok(bytes) => return Ok(bytes),
        Err(original) => {
            let mut at = PathBuf::new();
            for part in path.components() {
                match part {
                    // Preserve platform prefix/root and OS parent-directory
                    // semantics. Relative paths start from the current directory.
                    Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir => at.push(part.as_os_str()),
                    Component::Normal(want) => {
                        let base = if at.as_os_str().is_empty() { Path::new(".") } else { &at };
                        let want = want.to_string_lossy();
                        let next = std::fs::read_dir(base).map_err(|_| original.clone())?
                            .flatten().map(|e| e.path()).find(|p| p.file_name()
                                .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&want)))
                            .ok_or_else(|| original.clone())?;
                        at = next;
                    }
                }
            }
            crate::resources::read_file(&at)
        }
    }
}

/// Write a complete NKA beside its destination, then atomically replace it.
/// A failed write never truncates the previous preset or creates its parent.
pub(super) fn save_nka(path: &str, ty: Ty, name: &str, value: &Value) -> Result<(), String> {
    use std::{fmt::Write as _, io::Write as _, path::Path, sync::atomic::{AtomicU64, Ordering}};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = Path::new(path);
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let filename = path.file_name().ok_or("save_array_str: destination has no filename")?;
    let sigil = match ty { Ty::Int => '%', Ty::Real => '?', Ty::Str => '!' };
    let mut text = format!("{sigil}{name}\n");
    let mut emit = |value: &Value| -> Result<(), String> {
        match (ty, value) {
            (Ty::Int, Value::Int(n)) => writeln!(text, "{n}").unwrap(),
            (Ty::Real, Value::Real(n)) if n.is_finite() => writeln!(text, "{n}").unwrap(),
            (Ty::Str, Value::Text(t)) if !t.contains(['\r', '\n']) => {
                text.push_str(t); text.push('\n');
            }
            (Ty::Str, Value::Text(_)) => return Err("save_array_str: NKA string contains a line break".into()),
            _ => return Err("save_array_str: invalid typed snapshot value".into()),
        }
        Ok(())
    };
    match value {
        Value::IntArray(values) if ty == Ty::Int => for n in values { emit(&Value::Int(*n))?; },
        Value::RealArray(values) if ty == Ty::Real => for n in values { emit(&Value::Real(*n))?; },
        Value::Array(values) => for value in values { emit(value)?; },
        _ => return Err("save_array_str: snapshot is not an array".into()),
    }
    // Respect an existing read-only file, and retain its permissions when replacing it.
    let permissions = match std::fs::metadata(path) {
        Ok(meta) if meta.permissions().readonly() => return Err("save_array_str: destination is read-only".into()),
        Ok(meta) => Some(meta.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("save_array_str: destination metadata: {error}")),
    };
    let temporary = parent.join(format!(".{}.kontra-{}-{}.tmp", filename.to_string_lossy(),
        std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)
        .map_err(|e| format!("save_array_str: create temporary file: {e}"))?;
    let result = (|| {
        file.write_all(text.as_bytes()).map_err(|e| format!("save_array_str: write file: {e}"))?;
        file.flush().map_err(|e| format!("save_array_str: flush file: {e}"))?;
        if let Some(permissions) = permissions { file.set_permissions(permissions)
            .map_err(|e| format!("save_array_str: preserve permissions: {e}"))?; }
        file.sync_all().map_err(|e| format!("save_array_str: sync file: {e}"))?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|e| format!("save_array_str: replace file: {e}"))
    })();
    if result.is_err() { let _ = std::fs::remove_file(temporary); }
    result
}

/// Typed NKA arrays: the exact variable header, then one value per line.
/// Invalid numeric data is rejected rather than silently replacing a preset
/// cell with zero. NI documents typed arrays, not malformed-file coercion.
pub(super) fn nka(bytes: &[u8], ty: Ty, name: &str) -> Option<Value> { parse_nka(bytes, ty, name).ok() }

fn parse_nka(bytes: &[u8], ty: Ty, name: &str) -> Result<Value, &'static str> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines().map(|l| l.strip_suffix('\r').unwrap_or(l));
    let head = lines.next().ok_or("load_array_str: missing NKA array header")?.trim();
    let sigil = match ty { Ty::Int => '%', Ty::Real => '?', Ty::Str => '!' };
    if head != name && head.strip_prefix(sigil) != Some(name) {
        return Err("load_array_str: NKA array header does not match destination name and type");
    }
    let values = lines.map(|line| match ty {
        Ty::Int => line.trim().parse::<i32>().map(Value::Int)
            .map_err(|_| "load_array_str: invalid NKA integer value"),
        Ty::Real => line.trim().parse::<f64>().ok().filter(|n| n.is_finite()).map(Value::Real)
            .ok_or("load_array_str: invalid or non-finite NKA real value"),
        Ty::Str => Ok(Value::Text(line.to_owned())),
    }).collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(values))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nka_preserves_numeric_and_string_arrays_and_rejects_other_names() {
        assert_eq!(nka(b"%values\r\n-4\r\n42\r\n", Ty::Int, "values"),
            Some(Value::Array(vec![Value::Int(-4), Value::Int(42)])));
        assert_eq!(nka(b"?values\n0.25\n-1.5\n", Ty::Real, "values"),
            Some(Value::Array(vec![Value::Real(0.25), Value::Real(-1.5)])));
        assert_eq!(nka("!names\nFirst name\n😀 second\n\n".as_bytes(), Ty::Str, "names"),
            Some(Value::Array(vec![Value::Text("First name".into()), Value::Text("😀 second".into()), Value::Text("".into())])));
        assert_eq!(nka(b"values\n42\n", Ty::Int, "values"), Some(Value::Array(vec![Value::Int(42)])));
        assert_eq!(nka(b"%other\n42\n", Ty::Int, "values"), None);
        assert_eq!(nka(b"!values\n42\n", Ty::Int, "values"), None);
        assert!(parse_nka(b"%values\nnot-a-number\n", Ty::Int, "values").unwrap_err().contains("integer"));
        assert!(parse_nka(b"?values\nNaN\n", Ty::Real, "values").unwrap_err().contains("non-finite"));
        assert!(parse_nka(b"?values\n1e999\n", Ty::Real, "values").is_err());
    }

    #[test]
    fn nka_writes_typed_files_and_preserves_existing_bytes_on_failure() {
        let root = std::env::temp_dir().join(format!("kontra-nka-write-{}",std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let cases = [
            (Ty::Int,Value::IntArray(vec![-4,42]),b"%values\n-4\n42\n".to_vec()),
            (Ty::Real,Value::RealArray(vec![0.25,-1.5]),b"?values\n0.25\n-1.5\n".to_vec()),
            (Ty::Str,Value::Array(vec![Value::Text("Ω".into()),Value::Text("😀".into())]),"!values\nΩ\n😀\n".as_bytes().to_vec()),
        ];
        let path = root.join("values.nka");
        for (ty,value,expected) in cases {
            save_nka(&path.to_string_lossy(),ty,"values",&value).unwrap();
            assert_eq!(std::fs::read(&path).unwrap(),expected);
            assert!(parse_nka(&expected,ty,"values").is_ok());
        }
        let before = std::fs::read(&path).unwrap();
        assert!(save_nka(&path.to_string_lossy(),Ty::Str,"values",
            &Value::Array(vec![Value::Text("two\nlines".into())])).unwrap_err().contains("line break"));
        assert_eq!(std::fs::read(&path).unwrap(),before);
        let permissions = std::fs::metadata(&path).unwrap().permissions();
        let mut readonly = permissions.clone(); readonly.set_readonly(true);
        std::fs::set_permissions(&path,readonly).unwrap();
        assert!(save_nka(&path.to_string_lossy(),Ty::Int,"values",&Value::IntArray(vec![9]))
            .unwrap_err().contains("read-only"));
        assert_eq!(std::fs::read(&path).unwrap(),before);
        std::fs::set_permissions(&path,permissions).unwrap();
        assert!(save_nka(&root.join("missing/values.nka").to_string_lossy(),Ty::Int,"values",&Value::IntArray(vec![9]))
            .unwrap_err().contains("create temporary"));
        assert!(!root.join("missing").exists());
        std::fs::create_dir(root.join("directory")).unwrap();
        assert!(save_nka(&root.join("directory").to_string_lossy(),Ty::Int,"values",&Value::IntArray(vec![9]))
            .unwrap_err().contains("replace file"));
        assert!(!std::fs::read_dir(&root).unwrap().flatten().any(|e|e.file_name().to_string_lossy().ends_with(".tmp")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn array_path_fallback_preserves_relative_parents_and_platform_roots() {
        let root = std::path::PathBuf::from(format!(".kontra-nka-path-{}", std::process::id()));
        std::fs::create_dir_all(root.join("Data")).unwrap();
        std::fs::write(root.join("Data/Mixed.nka"), b"%values\n42\n").unwrap();
        for path in [
            root.join("data/MIXED.nka"),
            root.join("data/../DATA/mixed.nka"),
            std::env::current_dir().unwrap().join(&root).join("data/MIXED.nka"),
        ] {
            assert_eq!(read_file_path(&path.to_string_lossy()).unwrap(), b"%values\n42\n");
        }
        let error = read_file_path(&root.join("missing.nka").to_string_lossy()).unwrap_err();
        assert!(error.contains("missing.nka"), "{error}");
        std::fs::remove_dir_all(root).unwrap();
    }
}
