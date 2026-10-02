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
}

impl ArrayRead {
    pub(super) fn prepared(slot: u8, var: VarId, info: &Var) -> Self {
        Self { slot, id: -1, var, path: String::with_capacity(PATH_BYTES),
            name: info.name.trim_start_matches(['%', '!', '?', '$', '@', '~']).into(),
            ty: info.ty, len: info.len.unwrap_or(0) as usize, values: None,
            progress: 0, validated: false, success: false }
    }
    pub fn path(&self) -> &str { &self.path }
    /// Files and parse buffers are owned only by this non-audio operation.
    pub fn read(&mut self) -> bool {
        self.values = read_path(&self.path).and_then(|b| nka(&b, self.ty, &self.name));
        if let Some(Value::Array(values)) = &mut self.values { values.truncate(self.len); }
        self.validated = self.ty != Ty::Str;
        self.success = self.values.is_some();
        self.success
    }
    /// Called by the worker after the audio thread has installed the result.
    pub fn recycle(&mut self) {
        self.values = None;
        self.path.clear(); self.id = -1; self.progress = 0; self.validated = false; self.success = false;
    }
}

/// A file a script names, matching names without case when the exact
/// path is not there (libraries are made on case-insensitive systems).
pub(super) fn read_path(path: &str) -> Option<Vec<u8>> {
    let path = std::path::Path::new(path);
    if let Ok(bytes) = crate::resources::read_file(path) {
        return Some(bytes);
    }
    let mut at = std::path::PathBuf::from("/");
    for part in path.components().skip(1) {
        let want = part.as_os_str().to_string_lossy();
        let next = std::fs::read_dir(&at).ok()?.flatten().map(|e| e.path()).find(|p| {
            p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(&want))
        })?;
        at = next;
    }
    crate::resources::read_file(&at).ok()
}

/// An `.nka` file's values for an array of type `ty` named `name`: the
/// array's name, then one value per line. `None` when the name differs.
pub(super) fn nka(bytes: &[u8], ty: Ty, name: &str) -> Option<Value> {
    let text = String::from_utf8_lossy(bytes);
    let mut lines = text.lines().map(|l| l.strip_suffix('\r').unwrap_or(l));
    let head = lines.next()?.trim();
    if head.trim_start_matches(['%', '!', '?', '$', '@', '~']) != name {
        return None;
    }
    let value = |l: &str| match ty {
        Ty::Int => Value::Int(l.trim().parse().unwrap_or(0)),
        Ty::Real => Value::Real(l.trim().parse().unwrap_or(0.0)),
        Ty::Str => Value::Text(l.to_owned()),
    };
    Some(Value::Array(lines.map(value).collect()))
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
        assert_eq!(nka(b"%other\n42\n", Ty::Int, "values"), None);
    }
}
