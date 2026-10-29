//! The state a script saves (`onSave`), written as a constant Lua table into
//! a buffer the caller gives (no allocation), for [`crate::literal`] to read
//! back as the `__state` module the next load passes to `onLoad`.

use std::io::Write;

use mlua::{Function, Lua, Value};

/// Tables nested deeper than this are cut: their value is `nil`.
const DEPTH: usize = 16;

/// Runs `onSave` and writes what it returns into `out`; the bytes written,
/// or `None` without an `onSave`, on its error, or when `out` is too small.
pub(crate) fn state(lua: &Lua, out: &mut [u8]) -> Option<usize> {
    let save = lua.globals().raw_get::<Function>("onSave").ok()?;
    let value = save.call::<Value>(()).ok()?;
    let len = out.len();
    let mut w: &mut [u8] = out;
    write(&mut w, &value, 0).ok()?;
    Some(len - w.len())
}

fn write(w: &mut &mut [u8], v: &Value, depth: usize) -> std::io::Result<()> {
    match v {
        Value::Boolean(b) => write!(w, "{b}"),
        Value::Integer(i) => write!(w, "{i}"),
        Value::Number(n) if n.is_finite() => write!(w, "{n}"),
        Value::String(s) => {
            w.write_all(b"\"")?;
            for &b in s.as_bytes().iter() {
                match b {
                    b'"' | b'\\' => w.write_all(&[b'\\', b])?,
                    b'\n' => w.write_all(b"\\n")?,
                    b => w.write_all(&[b])?,
                }
            }
            w.write_all(b"\"")
        }
        Value::Table(t) if depth < DEPTH => {
            w.write_all(b"{")?;
            for pair in t.pairs::<Value, Value>() {
                let Ok((k, v)) = pair else { continue };
                if !matches!(k, Value::Integer(_) | Value::Number(_) | Value::String(_)) {
                    continue;
                }
                w.write_all(b"[")?;
                write(w, &k, depth + 1)?;
                w.write_all(b"]=")?;
                write(w, &v, depth + 1)?;
                w.write_all(b",")?;
            }
            w.write_all(b"}")
        }
        _ => w.write_all(b"nil"),
    }
}
