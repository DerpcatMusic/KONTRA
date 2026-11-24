//! Finding library folders the player never named: where Native Instruments
//! and UVI registered or install them (the registry, Service Center XMLs,
//! plists, Wine prefixes, UVI's prefs), and a parallel walk of the whole
//! machine that matches folders by file extension alone, without opening any
//! file.
//!
//! Every function here touches the disk and must run on the scan thread.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Instant;

use crate::{Progress, Root};

// ---------------------------------------------------------------------------
// The parallel walk.

/// Runs `visit` on every task, starting from `starts`, on `threads` threads.
/// Each call returns the tasks it found (sub-folders). The walk ends when no
/// task is left and no thread is busy. Work is taken last-in first-out, so
/// it runs depth-first and holds little memory.
// ponytail: a single shared stack. Use per-thread deques with work stealing
// if lock contention ever shows up in a profile.
pub(crate) fn par_walk<T: Send>(
    starts: Vec<T>,
    threads: usize,
    visit: impl Fn(T) -> Vec<T> + Sync,
) {
    struct Queue<T> {
        work: Vec<T>,
        busy: usize,
    }
    let queue = Mutex::new(Queue {
        work: starts,
        busy: 0,
    });
    let wake = Condvar::new();
    let lock = || queue.lock().unwrap_or_else(PoisonError::into_inner);
    std::thread::scope(|s| {
        for _ in 0..threads.max(1) {
            s.spawn(|| {
                loop {
                    let task = {
                        let mut q = lock();
                        loop {
                            if let Some(task) = q.work.pop() {
                                q.busy += 1;
                                break task;
                            }
                            if q.busy == 0 {
                                wake.notify_all();
                                return;
                            }
                            q = wake.wait(q).unwrap_or_else(PoisonError::into_inner);
                        }
                    };
                    let more = visit(task);
                    let mut q = lock();
                    q.busy -= 1;
                    let any = !more.is_empty();
                    q.work.extend(more);
                    if any || q.busy == 0 {
                        wake.notify_all();
                    }
                }
            });
        }
    });
}

/// How many threads a walk uses. Disks rarely gain much past 8.
#[must_use]
pub fn threads() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get().min(8))
}

// ---------------------------------------------------------------------------
// The machine scan.

/// Limits on a machine walk. `Default` means no limit.
#[derive(Clone, Copy, Debug, Default)]
pub struct Limits {
    /// The deepest level below a start that is listed.
    pub depth: Option<usize>,
    /// When to stop.
    pub deadline: Option<Instant>,
    /// The most folders to list.
    pub folders: Option<usize>,
}

/// A library folder that the walk matched.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hit {
    pub dir: PathBuf,
    /// A root scan of `dir` finds it without help: it is strongly marked
    /// (`.nicnt`, `Instruments`, `Multis`, presets beside `Samples` or a
    /// monolith) or it holds banks.
    pub standalone: bool,
}

/// Preset and bank extensions that mark a library. Nothing is opened.
const PRESETS: [&str; 7] = ["nki", "nkm", "nksn", "uvip", "sfz", "dspreset", "kontra"];
const BANKS: [&str; 3] = ["ufs", "sf2", "sf3"];
const MONOLITHS: [&str; 3] = ["nkx", "nkc", "nkr"];
const AUDIO: [&str; 6] = ["wav", "ncw", "aif", "aiff", "flac", "ogg"];

/// Folder names that never hold libraries, at any depth (lowercase).
const JUNK: [&str; 16] = [
    "node_modules",
    "__pycache__",
    "$recycle.bin",
    "system volume information",
    "windows",
    "programdata",
    "appdata",
    "caches",
    "cache",
    "containers",
    "group containers",
    "developer",
    "lost+found",
    "samples",
    "proc",
    "steamapps",
];

/// System folders a machine walk skips. Starts are always walked, so a
/// drive mounted under `/run/media` is still searched.
const SYSTEM: [&str; 22] = [
    "/proc",
    "/sys",
    "/dev",
    "/run",
    "/usr",
    "/nix",
    "/gnu",
    "/bin",
    "/sbin",
    "/lib",
    "/lib32",
    "/lib64",
    "/etc",
    "/boot",
    "/efi",
    "/var",
    "/tmp",
    "/snap",
    "/System",
    "/private",
    "/cores",
    "/Applications",
];

fn ext_of(name: &str) -> String {
    name.rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// Inside a `Program Files` folder, only NI's and UVI's folders are walked.
fn program_files_child(parent: &Path, name: &str) -> bool {
    let p = parent
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if !p.starts_with("program files") {
        return true;
    }
    name.contains("native instruments") || name.contains("uvi")
}

/// Every library folder under `starts`. Symlinked folders are not followed,
/// and a folder is listed only once, so overlapping starts cost nothing
/// extra. The walk stops descending at a strongly marked library, because
/// the catalog scan walks inside it. A canceled `progress` ends the walk early with what it found.
#[must_use]
pub fn walk(starts: &[PathBuf], limits: Limits, progress: &Progress) -> Vec<Hit> {
    let seen = Mutex::new(HashSet::<PathBuf>::new());
    let hits = Mutex::new(Vec::new());
    let over = || {
        progress.canceled()
            || limits.deadline.is_some_and(|d| Instant::now() >= d)
            || limits
                .folders
                .is_some_and(|n| progress.folders.load(Ordering::Relaxed) >= n)
    };
    let tasks: Vec<(PathBuf, usize)> = starts.iter().map(|s| (s.clone(), 0)).collect();
    par_walk(tasks, threads(), |(dir, depth)| {
        if over() {
            return Vec::new();
        }
        if !seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(dir.clone())
        {
            return Vec::new();
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        progress.folders.fetch_add(1, Ordering::Relaxed);
        let (mut nicnt, mut instruments, mut samples, mut monolith) = (false, false, false, false);
        let (mut presets, mut banks, mut audio) = (0usize, 0usize, 0usize);
        let mut folders = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.starts_with('.') {
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                match name.as_str() {
                    "instruments" | "multis" => instruments = true,
                    "samples" => samples = true,
                    _ => {}
                }
                let path = entry.path();
                let system = SYSTEM.iter().any(|s| path == Path::new(s));
                if !JUNK.contains(&name.as_str()) && !system && program_files_child(&dir, &name) {
                    folders.push(path);
                }
                continue;
            }
            // A symlinked file is matched by its name; a symlinked folder
            // is not followed.
            let e = ext_of(&name);
            match e.as_str() {
                "nicnt" => nicnt = true,
                e if MONOLITHS.contains(&e) => monolith = true,
                e if BANKS.contains(&e) => banks += 1,
                e if PRESETS.contains(&e) => presets += 1,
                e if AUDIO.contains(&e) => audio += 1,
                _ => {}
            }
        }
        let strong = nicnt || instruments || (presets > 0 && (samples || monolith));
        if strong || presets > 0 || banks > 0 {
            progress.found.fetch_add(1, Ordering::Relaxed);
            (hits.lock().unwrap_or_else(PoisonError::into_inner)).push(Hit {
                dir: dir.clone(),
                standalone: strong || banks > 0,
            });
            // A strong library is the catalog scan's to search inside. A
            // folder of loose presets may still hold libraries.
            if strong {
                return Vec::new();
            }
        }
        if audio >= crate::SAMPLE_ONLY || limits.depth.is_some_and(|d| depth >= d) {
            return Vec::new();
        }
        folders.sort();
        // Reversed, so the stack pops them in name order.
        folders.into_iter().rev().map(|f| (f, depth + 1)).collect()
    });
    let mut hits = hits.into_inner().unwrap_or_else(PoisonError::into_inner);
    hits.sort();
    hits
}

/// The roots to scan for `hits`. Libraries that share a folder become one
/// root of that folder. A library whose folder is a start (home, a drive) is
/// its own root, so the whole drive is not scanned again. Loose files in a
/// start itself are ignored. Roots inside another root are dropped, because
/// the catalog scan has no depth limit.
// ponytail: loose banks directly in home or at a drive's root are not
// listed. Add each bank as a file root if players keep them there.
#[must_use]
pub fn group(hits: &[Hit], starts: &[PathBuf]) -> Vec<Root> {
    let mut roots: Vec<Root> = hits
        .iter()
        .filter(|hit| !starts.contains(&hit.dir))
        .map(|hit| match hit.dir.parent() {
            Some(parent) if !starts.iter().any(|s| s == parent) => Root {
                path: parent.to_path_buf(),
                single: false,
                auto: true,
            },
            _ => Root {
                path: hit.dir.clone(),
                single: !hit.standalone,
                auto: true,
            },
        })
        .collect();
    roots.sort_by(|a, b| a.path.cmp(&b.path));
    roots.dedup_by(|a, b| a.path == b.path);
    nest(roots, &[])
}

/// `roots` without those that `others`, or another of `roots`, already cover:
/// the same path, or a path inside a folder root.
#[must_use]
pub fn nest(roots: Vec<Root>, others: &[Root]) -> Vec<Root> {
    let covers = |r: &Root, p: &Path| !r.single && p != r.path && p.starts_with(&r.path);
    roots
        .iter()
        .filter(|r| {
            !others
                .iter()
                .any(|o| o.path == r.path || covers(o, &r.path))
                && !roots.iter().any(|o| covers(o, &r.path))
        })
        .cloned()
        .collect()
}

/// Where a machine scan starts: the home folder and every mounted volume.
#[must_use]
pub fn starts() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = dirs::home_dir().into_iter().collect();
    if cfg!(target_os = "linux") {
        if let Ok(mounts) = std::fs::read_to_string("/proc/self/mounts") {
            out.extend(mount_points(&mounts));
        }
        let user = std::env::var("USER").unwrap_or_default();
        for parent in [
            "/mnt".to_owned(),
            "/media".to_owned(),
            format!("/media/{user}"),
            format!("/run/media/{user}"),
        ] {
            out.extend(children(Path::new(&parent)));
        }
    } else if cfg!(target_os = "macos") {
        out.push("/Users/Shared".into());
        out.push("/Library/Application Support".into());
        out.extend(children(Path::new("/Volumes")));
    } else if cfg!(windows) {
        out.extend(
            (b'A'..=b'Z')
                .map(|l| PathBuf::from(format!("{}:\\", l as char)))
                .filter(|p| p.is_dir()),
        );
    }
    let mut seen = BTreeSet::new();
    out.retain(|p| p.is_dir() && seen.insert(p.clone()));
    out
}

fn children(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir)
        .map(|e| {
            e.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default()
}

/// The mount points in `/proc/self/mounts` text that hold disk filesystems.
#[must_use]
pub fn mount_points(mounts: &str) -> Vec<PathBuf> {
    const DISK: [&str; 13] = [
        "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "zfs", "ntfs", "ntfs3", "fuseblk", "exfat",
        "vfat", "hfsplus",
    ];
    mounts
        .lines()
        .filter_map(|line| {
            let mut f = line.split_whitespace();
            let (_dev, at, fs) = (f.next()?, f.next()?, f.next()?);
            let at = at.replace("\\040", " ");
            let system = at.starts_with("/boot")
                || at.starts_with("/efi")
                || (SYSTEM
                    .iter()
                    .any(|s| at == *s || at.starts_with(&format!("{s}/")))
                    && !at.starts_with("/run/media"));
            (DISK.contains(&fs) && !system).then(|| PathBuf::from(at))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Registrations and default folders.

/// Folders that Native Instruments and UVI registered, or that they install
/// to by default, and that exist. These are cheap to find: a few files read,
/// plus `reg`/`plutil` on Windows and macOS.
#[must_use]
pub fn registered() -> Vec<PathBuf> {
    let home = dirs::home_dir().unwrap_or_default();
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        for hive in ["HKLM", "HKCU"] {
            let key = format!(r"{hive}\SOFTWARE\Native Instruments");
            if let Some(text) = run("reg", &["query", &key, "/s", "/v", "ContentDir"]) {
                out.extend(reg_query(&text).into_iter().map(PathBuf::from));
            }
        }
        let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
        for common in [env("CommonProgramFiles"), env("CommonProgramFiles(x86)")]
            .into_iter()
            .flatten()
        {
            out.extend(
                xml_dirs(&common.join(r"Native Instruments\Service Center"))
                    .into_iter()
                    .map(PathBuf::from),
            );
        }
        let public = env("PUBLIC").unwrap_or_else(|| r"C:\Users\Public".into());
        let program = env("ProgramFiles").unwrap_or_else(|| r"C:\Program Files".into());
        out.extend([
            public.join(r"Documents\Native Instruments"),
            public.join(r"Documents\UVI Soundbanks"),
            program.join("Native Instruments"),
            program.join("UVISoundBanks"),
            home.join(r"Documents\Native Instruments"),
            home.join(r"Documents\UVI Soundbanks"),
        ]);
        if let Some(app) = env("APPDATA") {
            out.extend(uvi_pref_dirs(&app.join("UVI"), None));
        }
    }
    #[cfg(target_os = "macos")]
    {
        let sc = Path::new("/Library/Application Support/Native Instruments/Service Center");
        out.extend(xml_dirs(sc).iter().flat_map(|d| hfs_path(d)));
        for prefs in [
            Path::new("/Library/Preferences").to_path_buf(),
            home.join("Library/Preferences"),
        ] {
            for plist in files(&prefs, |n| {
                n.starts_with("com.native-instruments.") && n.ends_with(".plist")
            }) {
                let path = plist.to_string_lossy().into_owned();
                if let Some(xml) = run("plutil", &["-convert", "xml1", "-o", "-", &path]) {
                    out.extend(plist_dirs(&xml).iter().flat_map(|d| hfs_path(d)));
                }
            }
        }
        out.extend([
            PathBuf::from("/Users/Shared"),
            "/Library/Application Support/Native Instruments".into(),
            "/Library/Application Support/UVISoundBanks".into(),
            home.join("Documents/Native Instruments"),
            home.join("Documents/UVI Soundbanks"),
            home.join("Music/UVI Soundbanks"),
        ]);
        out.extend(uvi_pref_dirs(
            &home.join("Library/Application Support/UVI"),
            None,
        ));
        out.extend(uvi_pref_dirs(&home.join("Library/Preferences"), None));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        out.extend([
            home.join("Documents/Native Instruments"),
            home.join("Documents/UVI Soundbanks"),
            home.join("UVI Soundbanks"),
            home.join("UVISoundBanks"),
        ]);
        for prefix in wine_prefixes(&home) {
            out.extend(in_prefix(&prefix));
        }
    }
    let mut seen = BTreeSet::new();
    out.retain(|p| p.is_dir() && seen.insert(p.clone()));
    out
}

/// What a command printed, when it ran and succeeded. Windows hides the
/// console window.
#[cfg(any(windows, target_os = "macos"))]
fn run(program: &str, args: &[&str]) -> Option<String> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The files directly in `dir` whose lowercase name passes `keep`.
fn files(dir: &Path, keep: impl Fn(&str) -> bool) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|e| {
            e.flatten()
                .filter(|e| keep(&e.file_name().to_string_lossy().to_lowercase()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The `ContentDir` values of every Service Center XML in `dir`.
fn xml_dirs(dir: &Path) -> Vec<String> {
    files(dir, |n| n.ends_with(".xml"))
        .iter()
        .filter_map(|f| std::fs::read(f).ok())
        .flat_map(|b| service_center_xml(&String::from_utf8_lossy(&b)))
        .collect()
}

/// Wine prefixes under `home`: `$WINEPREFIX`, `~/.wine`, winetricks', Bottles'
/// (native and Flatpak) and Lutris' (`~/Games/*`). A prefix has a `system.reg`.
#[cfg(all(unix, not(target_os = "macos")))]
fn wine_prefixes(home: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::env::var_os("WINEPREFIX")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    out.push(home.join(".wine"));
    for parent in [
        ".local/share/wineprefixes",
        ".local/share/bottles/bottles",
        ".var/app/com.usebottles.bottles/data/bottles/bottles",
        "Games",
    ] {
        for dir in children(&home.join(parent)) {
            out.push(dir.join("prefix"));
            out.push(dir);
        }
    }
    let mut seen = BTreeSet::new();
    out.retain(|p| p.join("system.reg").is_file() && seen.insert(p.clone()));
    out
}

/// The NI and UVI folders that a Wine prefix registered or holds, as host paths.
#[cfg(all(unix, not(target_os = "macos")))]
fn in_prefix(prefix: &Path) -> Vec<PathBuf> {
    let mut wins = Vec::new();
    for reg in ["system.reg", "user.reg"] {
        if let Ok(b) = std::fs::read(prefix.join(reg)) {
            wins.extend(reg_file(&String::from_utf8_lossy(&b)));
        }
    }
    let c = prefix.join("drive_c");
    for pf in ["Program Files", "Program Files (x86)"] {
        wins.extend(xml_dirs(
            &c.join(pf)
                .join("Common Files/Native Instruments/Service Center"),
        ));
    }
    let mut out: Vec<PathBuf> = wins.iter().map(|w| wine_path(prefix, w)).collect();
    out.extend([
        c.join("users/Public/Documents/Native Instruments"),
        c.join("users/Public/Documents/UVI Soundbanks"),
        c.join("Program Files/Native Instruments"),
        c.join("Program Files/UVISoundBanks"),
    ]);
    for user in children(&c.join("users")) {
        out.push(user.join("Documents/Native Instruments"));
        out.push(user.join("Documents/UVI Soundbanks"));
        out.extend(uvi_pref_dirs(
            &user.join("AppData/Roaming/UVI"),
            Some(prefix),
        ));
    }
    out
}

/// Soundbank folders that UVI Workstation's or Falcon's prefs name: every
/// absolute path in the text files under `dir`, up to 3 levels deep, that is
/// a folder named like a soundbank folder or holding a `.ufs`. `prefix` maps
/// Windows paths in a Wine prefix to host paths.
// ponytail: a path scrape, not a parser. UVI's pref formats are
// undocumented. Parse them properly once one is known.
fn uvi_pref_dirs(dir: &Path, prefix: Option<&Path>) -> Vec<PathBuf> {
    let mut texts = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if meta.is_dir() && depth < 3 {
                stack.push((path, depth + 1));
            } else if meta.is_file()
                && meta.len() < 512 * 1024
                && (dir.ends_with("UVI") || name.contains("uvi"))
            {
                texts.extend(std::fs::read(&path).ok());
            }
        }
    }
    texts
        .iter()
        .flat_map(|b| paths_in(&String::from_utf8_lossy(b)))
        .map(|p| match prefix {
            Some(prefix) if !p.starts_with('/') => wine_path(prefix, &p),
            _ => PathBuf::from(p),
        })
        .filter(|p| {
            let name = p.to_string_lossy().to_lowercase();
            p.is_dir()
                && (name.contains("soundbank") || !files(p, |n| n.ends_with(".ufs")).is_empty())
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Parsers, pure and tested on fixture text.

/// `ContentDir` values from `reg query … /s /v ContentDir` output.
#[must_use]
pub fn reg_query(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("ContentDir")?.trim_start();
            let value = rest.strip_prefix("REG_SZ")?.trim();
            (!value.is_empty()).then(|| value.to_owned())
        })
        .collect()
}

/// `ContentDir` values in the Native Instruments keys of a Wine `.reg` file
/// (`system.reg`, `user.reg`), with `\\` unescaped.
#[must_use]
pub fn reg_file(text: &str) -> Vec<String> {
    let mut ni = false;
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            ni = line.to_ascii_lowercase().contains("native instruments");
        } else if ni {
            let Some(value) = line.strip_prefix("\"ContentDir\"=\"") else {
                continue;
            };
            let value = value.strip_suffix('"').unwrap_or(value);
            out.push(value.replace("\\\\", "\\"));
        }
    }
    out
}

/// Every `<ContentDir>` value in a Service Center XML.
#[must_use]
pub fn service_center_xml(text: &str) -> Vec<String> {
    text.split("<ContentDir>")
        .skip(1)
        .filter_map(|s| s.split_once("</ContentDir>"))
        .map(|(v, _)| unescape(v.trim()))
        .filter(|v| !v.is_empty())
        .collect()
}

/// The `ContentDir` `<string>` values in an XML plist.
#[must_use]
pub fn plist_dirs(text: &str) -> Vec<String> {
    text.split("<key>ContentDir</key>")
        .skip(1)
        .filter_map(|s| {
            let s = s.trim_start().strip_prefix("<string>")?;
            Some(unescape(s.split_once("</string>")?.0.trim()))
        })
        .collect()
}

fn unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// A Mac path as NI writes it: POSIX as it is, or HFS (`Macintosh
/// HD:Users:Shared:Lib:`) as both `/Volumes/<vol>/…` and `/…`, because the
/// boot volume's name is not known here.
#[must_use]
pub fn hfs_path(s: &str) -> Vec<PathBuf> {
    if s.starts_with('/') || !s.contains(':') {
        return vec![PathBuf::from(s)];
    }
    let mut parts = s.split(':').filter(|p| !p.is_empty());
    let Some(volume) = parts.next() else {
        return Vec::new();
    };
    let rest: PathBuf = parts.collect();
    vec![
        Path::new("/Volumes").join(volume).join(&rest),
        Path::new("/").join(rest),
    ]
}

/// A Windows path inside the Wine prefix `prefix`, as a host path. `Z:` is
/// `/`, `C:` is `drive_c`, and any other drive goes through `dosdevices`.
#[must_use]
pub fn wine_path(prefix: &Path, win: &str) -> PathBuf {
    let win = win.replace('\\', "/");
    let (drive, rest) = match win.as_bytes() {
        [d, b':', ..] if d.is_ascii_alphabetic() => (d.to_ascii_lowercase(), &win[2..]),
        _ => return PathBuf::from(win),
    };
    let rest = rest.trim_matches('/');
    let base = match drive {
        b'z' => PathBuf::from("/"),
        b'c' => prefix.join("drive_c"),
        d => prefix.join("dosdevices").join(format!("{}:", d as char)),
    };
    if rest.is_empty() {
        base
    } else {
        base.join(rest)
    }
}

/// Absolute paths in free text: POSIX (`/…`) or Windows (`X:\…`, `X:/…`),
/// cut at quotes, angle brackets, `=` and line ends.
#[must_use]
pub fn paths_in(text: &str) -> Vec<String> {
    text.split(['"', '<', '>', '=', '\n', '\r', '\t', '\0'])
        .map(str::trim)
        .filter(|t| {
            let b = t.as_bytes();
            (t.starts_with('/') && t.len() > 1)
                || matches!(b, [d, b':', b'\\' | b'/', ..] if d.is_ascii_alphabetic())
        })
        .map(|t| t.trim_end_matches(['/', '\\']).to_owned())
        .collect()
}

// ---------------------------------------------------------------------------
// Flatpak.

/// The Flatpak app id KONTRA runs inside, if any: `$FLATPAK_ID`, otherwise
/// `/.flatpak-info`.
#[must_use]
pub fn flatpak_app() -> Option<String> {
    std::env::var("FLATPAK_ID")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| flatpak_id(&std::fs::read_to_string("/.flatpak-info").ok()?))
}

/// The `name=` of the `[Application]` group of a `.flatpak-info`.
#[must_use]
pub fn flatpak_id(info: &str) -> Option<String> {
    let mut app = false;
    for line in info.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            app = line == "[Application]";
        } else if app && let Some(name) = line.strip_prefix("name=") {
            return Some(name.trim().to_owned());
        }
    }
    None
}

/// The command that lets Flatpak app `app` read `path`.
#[must_use]
pub fn flatpak_hint(path: &Path, app: &str) -> String {
    let path = path.display().to_string();
    let quoted = if path.contains([' ', '\'', '"']) {
        format!("'{}'", path.replace('\'', r"'\''"))
    } else {
        path
    };
    format!(
        "Sandboxed (Flatpak): run  flatpak override --user --filesystem={quoted} {app}  then restart the DAW."
    )
}

// ---------------------------------------------------------------------------
// Putting it together.

/// The roots found without asking: registrations and default folders, plus
/// a shallow walk of home and the mounted volumes. The walk lists at most
/// 3 levels, for at most 1.5 s or 40 000 folders.
#[must_use]
pub fn quick(progress: &Progress) -> Vec<Root> {
    let starts = starts();
    let limits = Limits {
        depth: Some(3),
        deadline: Some(Instant::now() + std::time::Duration::from_millis(1500)),
        folders: Some(40_000),
    };
    let found = Progress::default();
    let hits = walk(&starts, limits, &found);
    if progress.canceled() {
        return Vec::new();
    }
    let mut roots: Vec<Root> = registered()
        .into_iter()
        .map(|path| Root {
            path,
            single: false,
            auto: true,
        })
        .collect();
    roots.extend(group(&hits, &starts));
    roots.sort_by(|a, b| a.path.cmp(&b.path));
    roots.dedup_by(|a, b| a.path == b.path);
    nest(roots, &[])
}

/// The whole machine, walked with no limit; cancel it through `progress`.
#[must_use]
pub fn machine(progress: &Progress) -> Vec<Root> {
    let starts = starts();
    let hits = walk(&starts, Limits::default(), progress);
    group(&hits, &starts)
}

/// The player's `user` roots, then `auto` roots that are not hidden and not
/// already covered by another root.
#[must_use]
pub fn merge(user: &[Root], auto: &[Root], hidden: &[PathBuf]) -> Vec<Root> {
    let auto: Vec<Root> = auto
        .iter()
        .filter(|r| !hidden.contains(&r.path))
        .map(|r| Root {
            auto: true,
            ..r.clone()
        })
        .collect();
    let mut out = user.to_vec();
    out.extend(nest(auto, user));
    out
}
