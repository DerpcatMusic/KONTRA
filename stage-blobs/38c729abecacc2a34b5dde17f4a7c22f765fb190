//! The libraries Kontakt and Native Access know about, so a first run finds
//! them with nothing to set up.
//!
//! Every source is a file or a command that may be missing; each is looked
//! at only if it is there, and each folder it names is kept only if it is a
//! library folder on this machine. Read here: the registry (`reg query` on
//! Windows, the `.reg` hives of a Wine prefix elsewhere), the per-library
//! plists on macOS, Service Center product files, Native Access's own files,
//! and the folders libraries are installed to by default. Kontakt's
//! `komplete.db3` is not read: it is SQLite, and no SQLite reader is in the
//! tree.

use super::{list, visit, Progress, Root, DEPTH};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Registry values naming a library's folder.
const VALUES: [&str; 2] = ["contentdir", "installdir"];

/// The folders to add to `have` for the libraries Kontakt knows about.
pub fn roots(have: &[Root]) -> Vec<Root> {
    let mut named = Vec::new();
    let mut folders = Vec::new();
    let home = dirs::home_dir();
    let documents = dirs::document_dir();

    // Windows itself.
    #[cfg(windows)]
    {
        for key in [r"HKLM\SOFTWARE\Native Instruments", r"HKLM\SOFTWARE\WOW6432Node\Native Instruments"] {
            use std::os::windows::process::CommandExt;
            // No console window flashing up in a host.
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            let out = std::process::Command::new("reg")
                .args(["query", key, "/s"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
            if let Ok(out) = out {
                named.extend(reg_query(&String::from_utf8_lossy(&out.stdout)).into_iter().map(PathBuf::from));
            }
        }
        let program_data = std::env::var_os("ProgramData").map_or(PathBuf::from(r"C:\ProgramData"), PathBuf::from);
        let public = std::env::var_os("PUBLIC").map_or(PathBuf::from(r"C:\Users\Public"), PathBuf::from);
        let common = std::env::var_os("CommonProgramFiles")
            .map_or(PathBuf::from(r"C:\Program Files\Common Files"), PathBuf::from);
        named.extend(service_center(&common.join(r"Native Instruments\Service Center"), None));
        named.extend(service_center(&program_data.join(r"Native Instruments\Service Center"), None));
        named.extend(native_access(&public.join(r"Documents\Native Instruments\Native Access"), None));
        folders.push(public.join(r"Documents\Native Instruments"));
    }

    // macOS: a plist per library, and Service Center naming them.
    for prefs in [Some(PathBuf::from("/Library/Preferences")), home.as_ref().map(|h| h.join("Library/Preferences"))]
        .into_iter()
        .flatten()
    {
        for file in files(&prefs, "plist") {
            let name = file.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            if name.starts_with("com.native-instruments.") {
                named.extend(read(&file).map(|b| plist(&b)).unwrap_or_default().iter().filter_map(|p| place(p, None)));
            }
        }
    }
    named.extend(service_center(Path::new("/Library/Application Support/Native Instruments/Service Center"), None));
    folders.push(PathBuf::from("/Users/Shared"));

    // Wine: its registry hives, and the same folders inside its C: drive.
    #[cfg(not(windows))]
    {
        let prefix = std::env::var_os("WINEPREFIX").map(PathBuf::from).or_else(|| home.as_ref().map(|h| h.join(".wine")));
        if let Some(prefix) = prefix.filter(|p| p.join("drive_c").is_dir()) {
            for hive in ["system.reg", "user.reg"] {
                let paths = read(&prefix.join(hive)).map(|b| reg_file(&text(&b))).unwrap_or_default();
                named.extend(paths.iter().filter_map(|p| place(p, Some(&prefix))));
            }
            let c = prefix.join("drive_c");
            named.extend(service_center(
                &c.join("Program Files/Common Files/Native Instruments/Service Center"),
                Some(&prefix),
            ));
            named.extend(service_center(&c.join("ProgramData/Native Instruments/Service Center"), Some(&prefix)));
            named.extend(native_access(&c.join("users/Public/Documents/Native Instruments/Native Access"), Some(&prefix)));
            folders.push(c.join("users/Public/Documents/Native Instruments"));
            for user in dirs_in(&c.join("users")) {
                folders.push(user.join("Documents/Native Instruments"));
            }
        }
    }

    folders.extend(documents.map(|d| d.join("Native Instruments")));

    let libraries: Vec<PathBuf> = named.into_iter().filter(|d| is_library(d)).collect();
    let folders: Vec<PathBuf> = folders.into_iter().filter(|d| holds_libraries(d)).collect();
    merge(have, grouped(libraries, folders))
}

/// A folder is a library when it would be found as one by a scan.
fn is_library(dir: &Path) -> bool {
    let l = list(dir);
    l.nicnt.is_some() || l.instruments || l.presets > 0
}

fn holds_libraries(dir: &Path) -> bool {
    let mut found = Vec::new();
    dir.is_dir() && {
        visit(dir, 0, None, &mut found, &Progress::default());
        !found.is_empty()
    }
}

/// Libraries by their folders: two or more side by side are added as the
/// folder that holds them, so the list stays short; the rest one by one.
fn grouped(libraries: Vec<PathBuf>, folders: Vec<PathBuf>) -> Vec<Root> {
    let key = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let libraries: BTreeSet<PathBuf> = libraries.iter().map(|p| key(p)).collect();
    let mut by_parent: BTreeMap<PathBuf, usize> = BTreeMap::new();
    for dir in &libraries {
        if let Some(parent) = dir.parent().filter(|p| p.parent().is_some()) {
            *by_parent.entry(parent.to_path_buf()).or_default() += 1;
        }
    }
    let mut out: Vec<Root> = folders.iter().map(|f| root(&key(f), false)).collect();
    out.extend(by_parent.iter().filter(|(_, n)| **n > 1).map(|(p, _)| root(p, false)));
    for dir in &libraries {
        if dir.parent().is_none_or(|p| by_parent.get(p).is_none_or(|n| *n < 2)) {
            out.push(root(dir, true));
        }
    }
    out
}

fn root(path: &Path, single: bool) -> Root {
    Root { path: path.to_string_lossy().into_owned(), single }
}

/// `new` without what `have` has, or what a folder in either already holds
/// within reach of a scan.
fn merge(have: &[Root], new: Vec<Root>) -> Vec<Root> {
    let covered = |r: &Root, by: &Root| {
        let (r, by) = (Path::new(&r.path), Path::new(&by.path));
        r == by
    };
    let within = |r: &Root, by: &Root| {
        let (r, by) = (Path::new(&r.path), Path::new(&by.path));
        r != by && r.strip_prefix(by).is_ok_and(|rest| rest.components().count() <= DEPTH)
    };
    let mut out: Vec<Root> = Vec::new();
    for r in &new {
        let all = || have.iter().chain(new.iter());
        let known = have.iter().chain(out.iter()).any(|h| covered(r, h));
        let inside = all().any(|by| !by.single && within(r, by));
        if !known && !inside {
            out.push(r.clone());
        }
    }
    out
}

fn read(path: &Path) -> Option<Vec<u8>> {
    // Every file read here is small; one that is not is not what we want.
    (std::fs::metadata(path).ok()?.len() < 64 << 20).then(|| std::fs::read(path).ok()).flatten()
}

/// Text in UTF-16 with a byte order mark, as `regedit` exports, or UTF-8.
fn text(bytes: &[u8]) -> String {
    match bytes {
        [0xff, 0xfe, rest @ ..] => {
            let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        }
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(ext)))
        .collect()
}

#[cfg(not(windows))]
fn dirs_in(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()
}

/// The folders Service Center's product files in `dir` name, and the
/// plists of the products they name by registry key.
fn service_center(dir: &Path, wine: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for file in files(dir, "xml") {
        let Some(bytes) = read(&file) else { continue };
        let (paths, keys) = product_xml(&text(&bytes));
        out.extend(paths.iter().filter_map(|p| place(p, wine)));
        for key in keys {
            let plist_file = PathBuf::from(format!("/Library/Preferences/com.native-instruments.{key}.plist"));
            out.extend(read(&plist_file).map(|b| plist(&b)).unwrap_or_default().iter().filter_map(|p| place(p, None)));
        }
    }
    out
}

/// The folders Native Access's own files name: any absolute path in them.
fn native_access(dir: &Path, wine: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for file in files(dir, "json").into_iter().chain(files(dir, "xml")) {
        let Some(bytes) = read(&file) else { continue };
        let paths = match serde_json::from_slice(&bytes) {
            Ok(value) => json_paths(&value),
            Err(_) => product_xml(&text(&bytes)).0,
        };
        out.extend(paths.iter().filter_map(|p| place(p, wine)));
    }
    out
}

/// `ContentDir` and `InstallDir` under `Native Instruments` in a `.reg`
/// file: exported by `regedit`, or a Wine hive (`system.reg`, `user.reg`).
#[cfg_attr(windows, allow(dead_code))]
pub fn reg_file(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(key) = line.strip_prefix('[') {
            let key = key.rsplit_once(']').map_or(key, |(k, _)| k).to_lowercase().replace(r"\\", r"\");
            inside = [r"software\native instruments\", r"software\wow6432node\native instruments\"]
                .iter()
                .any(|k| key.contains(k));
            continue;
        }
        let Some((name, value)) = line.strip_prefix('"').and_then(|l| l.split_once("\"=")) else { continue };
        if !inside || !VALUES.contains(&name.to_lowercase().as_str()) {
            continue;
        }
        let value = value.trim_start_matches("str(2):").trim_start_matches("str(1):");
        if let Some(value) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) {
            out.push(unescape(value));
        }
    }
    out
}

/// A `.reg` string: `\\`, `\"` and Wine's `\x` escapes undone.
#[cfg_attr(windows, allow(dead_code))]
fn unescape(value: &str) -> String {
    let mut out = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('x') => {
                let mut code = String::new();
                while code.len() < 4 && chars.peek().is_some_and(char::is_ascii_hexdigit) {
                    code.extend(chars.next());
                }
                out.extend(u32::from_str_radix(&code, 16).ok().and_then(char::from_u32));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `ContentDir` and `InstallDir` in the output of `reg query <key> /s`.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn reg_query(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        for name in VALUES {
            let Some(rest) = line.get(..name.len()).filter(|n| n.eq_ignore_ascii_case(name)).map(|_| &line[name.len()..])
            else {
                continue;
            };
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix("REG_SZ").or_else(|| rest.strip_prefix("REG_EXPAND_SZ")) {
                let value = value.trim();
                if !value.is_empty() {
                    out.push(value.to_owned());
                }
            }
        }
    }
    out
}

/// A library plist's `ContentDir`: from the XML form, or, from the binary
/// form, every string that could be a path.
pub fn plist(bytes: &[u8]) -> Vec<String> {
    if bytes.starts_with(b"bplist00") {
        return bplist_strings(bytes)
            .unwrap_or_default()
            .into_iter()
            .filter(|s| s.starts_with('/') || s.contains(':'))
            .collect();
    }
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let mut rest = text.as_ref();
    while let Some(at) = rest.find("<key>") {
        rest = &rest[at + 5..];
        let Some((key, after)) = rest.split_once("</key>") else { break };
        if VALUES.contains(&key.trim().to_lowercase().as_str())
            && let Some(value) = after.trim_start().strip_prefix("<string>").and_then(|v| v.split_once("</string>")) {
                out.push(entities(value.0.trim()));
            }
    }
    out
}

/// The strings of a binary plist, found through its offset table.
fn bplist_strings(b: &[u8]) -> Option<Vec<String>> {
    let trailer = b.get(b.len().checked_sub(32)?..)?;
    let offset_size = usize::from(trailer[6]);
    let be = |bytes: &[u8]| bytes.iter().fold(0u64, |n, x| n << 8 | u64::from(*x));
    let count = usize::try_from(be(&trailer[8..16])).ok()?.min(100_000);
    let table = usize::try_from(be(&trailer[24..32])).ok()?;
    if !(1..=8).contains(&offset_size) {
        return None;
    }
    let mut out = Vec::new();
    for n in 0..count {
        let at = table.checked_add(n.checked_mul(offset_size)?)?;
        let Ok(offset) = usize::try_from(be(b.get(at..at.checked_add(offset_size)?)?)) else { continue };
        let Some(&marker) = b.get(offset) else { continue };
        let (kind, mut len, mut start) = (marker >> 4, usize::from(marker & 0xf), offset + 1);
        if kind != 5 && kind != 6 {
            continue;
        }
        if len == 0xf {
            // The length follows as an integer object.
            let Some(&int) = b.get(start) else { continue };
            let width = 1usize << (int & 0xf).min(3);
            let Some(bytes) = b.get(start + 1..start + 1 + width) else { continue };
            len = usize::try_from(be(bytes)).unwrap_or(usize::MAX);
            start += 1 + width;
        }
        let units = if kind == 6 { len.saturating_mul(2) } else { len };
        let Some(bytes) = start.checked_add(units).and_then(|end| b.get(start..end)) else { continue };
        out.push(if kind == 5 {
            bytes.iter().map(|&c| char::from(c)).collect()
        } else {
            let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&units)
        });
    }
    Some(out)
}

fn entities(text: &str) -> String {
    text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// From a Service Center product file: folders it names, and the registry
/// keys of its products.
pub fn product_xml(text: &str) -> (Vec<String>, Vec<String>) {
    let tag = |name: &str| -> Vec<String> {
        let (open, close) = (format!("<{name}>"), format!("</{name}>"));
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(at) = rest.find(&open) {
            rest = &rest[at + open.len()..];
            let Some((value, after)) = rest.split_once(&close) else { break };
            let value = entities(value.trim());
            if !value.is_empty() && value.len() < 400 {
                out.push(value);
            }
            rest = after;
        }
        out
    };
    let paths = ["ContentDir", "InstallDir", "ContentPath"].iter().flat_map(|t| tag(t)).collect();
    let keys = tag("RegKey")
        .into_iter()
        .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_alphanumeric() || " -_.".contains(c)))
        .collect();
    (paths, keys)
}

/// Every string in a JSON document that is an absolute path.
fn json_paths(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) if absolute(s) => vec![s.clone()],
        serde_json::Value::Array(items) => items.iter().flat_map(json_paths).collect(),
        serde_json::Value::Object(map) => map.values().flat_map(json_paths).collect(),
        _ => Vec::new(),
    }
}

fn absolute(s: &str) -> bool {
    s.len() < 400 && (s.starts_with('/') || drive(s).is_some())
}

/// The drive letter of `C:\…` or `C:/…`.
fn drive(s: &str) -> Option<char> {
    let mut chars = s.chars();
    let (letter, colon, slash) = (chars.next()?, chars.next()?, chars.next()?);
    (letter.is_ascii_alphabetic() && colon == ':' && (slash == '\\' || slash == '/')).then_some(letter)
}

/// Where a path from one of these files is on this machine: a Windows path
/// inside the Wine prefix it came from, a Mac `Volume:/path` or
/// `Volume:path:` on its volume, anything else as it is.
pub fn place(raw: &str, wine: Option<&Path>) -> Option<PathBuf> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(letter) = drive(raw) {
        let rest: Vec<&str> = raw[3..].split(['\\', '/']).filter(|s| !s.is_empty()).collect();
        return Some(match wine {
            Some(prefix) => {
                let devices = prefix.join("dosdevices").join(format!("{}:", letter.to_ascii_lowercase()));
                let base = if devices.exists() || !letter.eq_ignore_ascii_case(&'c') {
                    devices
                } else {
                    prefix.join("drive_c")
                };
                rest.iter().fold(base, |p, s| p.join(s))
            }
            None => PathBuf::from(raw.trim_end_matches(['\\', '/'])),
        });
    }
    if raw.starts_with('/') {
        return Some(PathBuf::from(if raw.len() > 1 { raw.trim_end_matches('/') } else { raw }));
    }
    // A Mac volume and a path on it.
    let (volume, rest) = raw.split_once(':')?;
    if volume.is_empty() {
        return None;
    }
    let rest = if rest.starts_with('/') { rest.to_owned() } else { format!("/{}", rest.replace(':', "/")) };
    let rest = rest.trim_end_matches('/');
    let mounted = PathBuf::from(format!("/Volumes/{volume}{rest}"));
    Some(if mounted.exists() { mounted } else { PathBuf::from(rest) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wine_hive_names_library_folders() {
        let hive = r#"WINE REGISTRY Version 2
;; All keys relative to \\Machine

[Software\\Native Instruments\\Areia] 1690000000
#time=1d9c0a7f5e4a3b2
"ContentDir"="C:\\Users\\Public\\Documents\\Areia\\"
"HU"="00000000"

[Software\\Wow6432Node\\Native Instruments\\Una Corda] 1690000001
"InstallDir"=str(2):"D:\\Libraries\\Una Corda\\"

[Software\\Native Instruments\\Caf\x00e9 Keys]
"ContentDir"="C:\\Libraries\\Caf\x00e9 Keys"

[Software\\Other Vendor\\Thing]
"ContentDir"="C:\\Not\\This"
"#;
        assert_eq!(
            reg_file(hive),
            [r"C:\Users\Public\Documents\Areia\", r"D:\Libraries\Una Corda\", r"C:\Libraries\Café Keys"]
        );
    }

    #[test]
    fn an_exported_reg_file_in_utf16_names_them_too() {
        let exported = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_LOCAL_MACHINE\\SOFTWARE\\Native Instruments\\Areia]\r\n\"ContentDir\"=\"E:\\\\Kontakt\\\\Areia\\\\\"\r\n";
        let mut bytes = vec![0xff, 0xfe];
        bytes.extend(exported.encode_utf16().flat_map(u16::to_le_bytes));
        assert_eq!(reg_file(&text(&bytes)), [r"E:\Kontakt\Areia\"]);
    }

    #[test]
    fn reg_query_output_names_them() {
        let out = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Native Instruments\\Areia\r\n    ContentDir    REG_SZ    E:\\Kontakt\\Areia\\\r\n    HU    REG_SZ    00\r\n    InstallDir64    REG_SZ    C:\\No\r\n\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Native Instruments\\Kontakt 7\r\n    InstallDir    REG_SZ    C:\\Program Files\\Native Instruments\\Kontakt 7\\\r\n";
        assert_eq!(reg_query(out), [r"E:\Kontakt\Areia\", r"C:\Program Files\Native Instruments\Kontakt 7\"]);
    }

    #[test]
    fn a_plist_names_its_content_folder() {
        let xml = br#"<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
  <key>ContentDir</key>
  <string>Macintosh HD:/Users/Shared/Areia &amp; More/</string>
  <key>ContentVersion</key><string>1.2.0</string>
</dict></plist>"#;
        assert_eq!(plist(xml), ["Macintosh HD:/Users/Shared/Areia & More/"]);
        // Binary: a dict of one key, its value in UTF-16 for the é.
        let path = "Macintosh HD:/Users/Shared/Café/";
        let mut b = b"bplist00".to_vec();
        let mut offsets = vec![b.len() as u8];
        b.extend([0xd1, 1, 2]);
        offsets.push(b.len() as u8);
        b.push(0x5a);
        b.extend(b"ContentDir");
        offsets.push(b.len() as u8);
        b.extend([0x6f, 0x10, path.chars().count() as u8]);
        b.extend(path.encode_utf16().flat_map(u16::to_be_bytes));
        let table = b.len() as u64;
        b.extend(&offsets);
        b.extend([0, 0, 0, 0, 0, 0, 1, 1]);
        b.extend(3u64.to_be_bytes());
        b.extend(0u64.to_be_bytes());
        b.extend(table.to_be_bytes());
        assert_eq!(plist(&b), [path]);
        assert!(plist(b"bplist00 truncated").is_empty());
    }

    #[test]
    fn service_center_files_give_folders_and_registry_keys() {
        let xml = "<ProductHints><Product version=\"1\"><Name>Areia</Name><RegKey>Areia</RegKey>\
                   <ContentDir>C:\\Libs\\Areia</ContentDir></Product>\
                   <Product><RegKey>../../etc</RegKey></Product></ProductHints>";
        assert_eq!(product_xml(xml), (vec![r"C:\Libs\Areia".to_owned()], vec!["Areia".to_owned()]));
    }

    #[test]
    fn native_access_json_gives_every_absolute_path() {
        let json: serde_json::Value = serde_json::from_str(
            r#"{"content":[{"name":"Areia","path":"D:\\Libs\\Areia"},{"path":"/Users/Shared/Una Corda"}],"v":"1.2"}"#,
        )
        .unwrap();
        assert_eq!(json_paths(&json), [r"D:\Libs\Areia", "/Users/Shared/Una Corda"]);
    }

    #[test]
    #[cfg(not(windows))] // Wine-prefix translation uses Unix paths, including d: symlinks.
    fn paths_land_where_they_are_on_this_machine() {
        let prefix = Path::new("/home/me/.wine");
        assert_eq!(place(r"C:\Users\Public\Areia\", Some(prefix)), Some(prefix.join("drive_c/Users/Public/Areia")));
        assert_eq!(place(r"D:\Libs\Areia", Some(prefix)), Some(prefix.join("dosdevices/d:/Libs/Areia")));
        assert_eq!(place("Macintosh HD:/Users/Shared/Areia/", None), Some(PathBuf::from("/Users/Shared/Areia")));
        assert_eq!(place("Macintosh HD:Users:Shared:Areia:", None), Some(PathBuf::from("/Users/Shared/Areia")));
        assert_eq!(place("/opt/libs/", None), Some(PathBuf::from("/opt/libs")));
        assert_eq!(place("relative", None), None);
    }

    #[test]
    fn found_libraries_are_grouped_and_not_added_twice() {
        let base = std::env::temp_dir().join(format!("kontra-kontakt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for lib in ["Vendor/A", "Vendor/B", "Lone/C"] {
            std::fs::create_dir_all(base.join(lib).join("Instruments")).unwrap();
        }
        let base = std::fs::canonicalize(&base).unwrap();
        let libs = ["Vendor/A", "Vendor/B", "Lone/C", "Missing/D"].map(|l| base.join(l));
        let found: Vec<PathBuf> = libs.into_iter().filter(|d| is_library(d)).collect();
        assert_eq!(found.len(), 3);
        let roots = grouped(found.clone(), Vec::new());
        assert_eq!(roots, [root(&base.join("Vendor"), false), root(&base.join("Lone/C"), true)]);
        // What the player has already, or holds in a folder of theirs, stays out.
        assert_eq!(merge(&[root(&base.join("Vendor"), false)], roots.clone()), [root(&base.join("Lone/C"), true)]);
        assert!(merge(&[root(&base, false)], roots).is_empty());
        let _ = std::fs::remove_dir_all(base);
    }
}
