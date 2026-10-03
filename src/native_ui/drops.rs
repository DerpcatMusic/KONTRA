//! Legacy drop payloads. Editor-only, binary-safe, with no fabricated formats.
use super::{Scene, invalid};
use anyhow::{Result, ensure};
use mlua::{UserData, UserDataFields, UserDataMethods};
use moose::mui::mui::{layout::Frame, prelude::Point};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};

const MAX_BYTES: usize = 1 << 20;
pub(super) fn valid_format(format: &str) -> bool {
    !format.is_empty() && format.len() <= 256 && !format.contains('\0')
}

#[derive(Clone)]
pub(crate) struct Payload {
    data: Arc<BTreeMap<String, Arc<[u8]>>>,
    pub(crate) origin: Option<(usize, u64, Weak<Mutex<Scene>>)>,
}
impl Payload {
    pub(crate) fn new(formats: impl IntoIterator<Item = (String, Arc<[u8]>)>) -> Result<Self> {
        let mut data = BTreeMap::new();
        let mut bytes = 0usize;
        for (format, value) in formats {
            ensure!(valid_format(&format), "Invalid native drop format");
            bytes = bytes
                .saturating_add(format.len())
                .saturating_add(value.len());
            ensure!(
                bytes <= MAX_BYTES && data.len() < 16,
                "Native drop payload exceeds its budget"
            );
            ensure!(
                data.insert(format, value).is_none(),
                "Duplicate native drop format"
            );
        }
        ensure!(!data.is_empty(), "Native drop payload has no formats");
        Ok(Self {
            data: Arc::new(data),
            origin: None,
        })
    }
    pub(crate) fn with_origin(
        mut self,
        part: usize,
        epoch: u64,
        scene: &Arc<Mutex<Scene>>,
    ) -> Self {
        self.origin = Some((part, epoch, Arc::downgrade(scene)));
        self
    }
    pub(super) fn matches(&self, formats: &[String]) -> bool {
        formats.iter().any(|format| self.data.contains_key(format))
    }
    /// Only actual absolute paths become text/uri-list. NI clipboard data is
    /// a different producer and is never synthesized from a filename.
    pub(crate) fn files(paths: &[PathBuf]) -> Result<Self> {
        ensure!(
            !paths.is_empty() && paths.len() <= 256,
            "Invalid native drop file count"
        );
        let mut uris = String::new();
        for path in paths {
            let uri = file_uri(path)?;
            ensure!(
                uris.len().saturating_add(uri.len()).saturating_add(2) <= MAX_BYTES - 13,
                "Native file drop exceeds its budget"
            );
            uris.push_str(&uri);
            uris.push_str("\r\n");
        }
        Self::new([("text/uri-list".into(), Arc::from(uris.into_bytes()))])
    }
}

pub(super) struct Event {
    pub payload: Payload,
    pub position: Point,
    pub frame: Frame,
}
impl UserData for Event {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("position", |lua, event| {
            let point = lua.create_table()?;
            point.set("x", event.position.x)?;
            point.set("y", event.position.y)?;
            Ok(point)
        });
        fields.add_field_method_get("frame", |lua, event| {
            let frame = lua.create_table()?;
            frame.set("x", event.frame.x)?;
            frame.set("y", event.frame.y)?;
            frame.set("width", event.frame.size.width)?;
            frame.set("height", event.frame.size.height)?;
            Ok(frame)
        });
    }
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("has_format", |_, event, format: String| {
            Ok(event.payload.data.contains_key(&format))
        });
        methods.add_method("data", |lua, event, format: String| {
            let bytes =
                event.payload.data.get(&format).ok_or_else(|| {
                    invalid(format!("Native drop has no data for format {format:?}"))
                })?;
            lua.create_string(bytes.as_ref())
        });
    }
}

fn escaped(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut result = String::new();
    for &byte in bytes {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            result.push(byte as char);
        } else {
            result.push('%');
            result.push(HEX[(byte >> 4) as usize] as char);
            result.push(HEX[(byte & 15) as usize] as char);
        }
    }
    result
}
fn file_uri(path: &Path) -> Result<String> {
    ensure!(
        path.is_absolute(),
        "Native file drop requires absolute paths"
    );
    // Bound before allocating an encoded path (worst-case three bytes each).
    ensure!(
        path.as_os_str().len() <= MAX_BYTES / 3 - 16,
        "Native file drop path exceeds its budget"
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        ensure!(
            !path.as_os_str().as_bytes().contains(&0),
            "Native file path contains a null byte"
        );
        let mut result = String::from("file://");
        for (index, part) in path
            .as_os_str()
            .as_bytes()
            .split(|byte| *byte == b'/')
            .enumerate()
        {
            if index > 0 {
                result.push('/');
            }
            result.push_str(&escaped(part));
        }
        return Ok(result);
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut parts = path.components();
        let Some(Component::Prefix(prefix)) = parts.next() else {
            anyhow::bail!("Unsupported native file path prefix");
        };
        let mut result = match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                format!("file:///{}:", drive as char)
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                let server = server
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Non-Unicode native UNC server"))?;
                ensure!(
                    !server.is_empty()
                        && server
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-._".contains(&b)),
                    "Unsupported native UNC authority"
                );
                format!(
                    "file://{server}/{}",
                    escaped(
                        share
                            .to_str()
                            .ok_or_else(|| anyhow::anyhow!("Non-Unicode native UNC share"))?
                            .as_bytes()
                    )
                )
            }
            _ => anyhow::bail!("Unsupported native file path prefix"),
        };
        for part in parts {
            let part = match part {
                Component::RootDir => continue,
                Component::Normal(value) => value,
                Component::CurDir => std::ffi::OsStr::new("."),
                Component::ParentDir => std::ffi::OsStr::new(".."),
                _ => anyhow::bail!("Unsupported native file path component"),
            };
            result.push('/');
            result.push_str(&escaped(
                part.to_str()
                    .ok_or_else(|| anyhow::anyhow!("Non-Unicode native file path"))?
                    .as_bytes(),
            ));
        }
        if result.ends_with(':') {
            result.push('/');
        }
        return Ok(result);
    }
    #[cfg(not(any(unix, windows)))]
    anyhow::bail!("Native file drop path encoding unavailable on this platform")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_uris_keep_path_bytes_and_do_not_invent_internal_clipboard_data() {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            assert_eq!(
                file_uri(Path::new("/tmp/space #/é.nki")).unwrap(),
                "file:///tmp/space%20%23/%C3%A9.nki"
            );
            let path = PathBuf::from(std::ffi::OsString::from_vec(
                b"/tmp/opaque\xff.nki".to_vec(),
            ));
            assert_eq!(file_uri(&path).unwrap(), "file:///tmp/opaque%FF.nki");
            let payload = Payload::files(&[PathBuf::from("/")]).unwrap();
            assert_eq!(payload.data.len(), 1);
            assert_eq!(payload.data["text/uri-list"].as_ref(), b"file:///\r\n");
        }
        assert!(file_uri(Path::new("relative.nki")).is_err());
        assert!(
            Payload::new((0..17).map(|index| (format!("x/{index}"), Arc::<[u8]>::from([]))))
                .is_err()
        );
        assert!(
            Payload::new([
                ("x".into(), Arc::<[u8]>::from([])),
                ("x".into(), Arc::<[u8]>::from([]))
            ])
            .is_err()
        );
    }
}
