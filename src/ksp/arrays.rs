//! Off-thread NKA reads. Requests and path storage are prepared before audio starts.
use super::{compile::{Ty, Var, VarId}, Value};

pub(super) const ARRAY_QUEUE: usize = 8;
const PATH_BYTES: usize = 4096;

/// An owned file job. Read/recycle it off the audio thread; installing its
/// result retains the job for worker disposal, never dropping strings on audio.
#[derive(Debug)]
pub struct ArrayRead {
    pub slot: u8,
    pub id: i32,
    pub(super) var: VarId,
    pub(super) path: String,
    name: Box<str>,
    ty: Ty,
    len: usize,
    pub(super) values: Option<Value>,
    pub(super) progress: usize,
    pub(super) validated: bool,
    pub(super) success: bool,
    pub(super) failure: Option<&'static str>,
    detail: Option<String>,
}

impl ArrayRead {
    pub(super) fn prepared(slot: u8, var: VarId, info: &Var) -> Self {
        Self { slot, id: -1, var, path: String::with_capacity(PATH_BYTES),
            name: info.name.trim_start_matches(['%', '!', '?', '$', '@', '~']).into(),
            ty: info.ty, len: info.len.unwrap_or(0) as usize, values: None,
            progress: 0, validated: false, success: false, failure: None, detail: None }
    }
    pub fn path(&self) -> &str { &self.path }
    /// Files and parse buffers are owned only by this non-audio operation.
    pub fn read(&mut self) -> bool {
        self.values = None;
        self.failure = None;
        self.detail = None;
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
        self.path.clear(); self.id = -1; self.progress = 0; self.validated = false; self.success = false;
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
