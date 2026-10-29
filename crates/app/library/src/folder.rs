//! A library's presets as the folders they sit in, for the browser's tree.

use std::path::{Path, PathBuf};

/// A folder of presets.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Folder {
    pub name: String,
    pub path: PathBuf,
    /// Its folders, sorted naturally by name.
    pub folders: Vec<Folder>,
    /// Its own presets, as indices into what the tree was built from.
    pub presets: Vec<usize>,
    /// Presets in it and every folder under it.
    pub count: usize,
}

impl Folder {
    /// The folders of `presets` under `dir`; a preset outside it sits at the
    /// top. A UVI bank is a folder like any other.
    pub fn tree<'a>(dir: &Path, presets: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut root = Folder {
            path: dir.to_path_buf(),
            ..Folder::default()
        };
        for (n, preset) in presets.into_iter().enumerate() {
            let inside = preset.parent().and_then(|p| p.strip_prefix(dir).ok());
            let mut at = &mut root;
            for part in inside.into_iter().flat_map(Path::components) {
                let name = part.as_os_str().to_string_lossy();
                let i = match at.folders.iter().rposition(|f| f.name == name) {
                    Some(i) => i,
                    None => {
                        let path = at.path.join(name.as_ref());
                        at.folders.push(Folder {
                            name: name.into_owned(),
                            path,
                            ..Folder::default()
                        });
                        at.folders.len() - 1
                    }
                };
                let Some(next) = at.folders.get_mut(i) else {
                    break;
                };
                at = next;
            }
            at.presets.push(n);
        }
        root.settle();
        root
    }

    /// Folders by name, numbers in them by value; counts summed.
    fn settle(&mut self) {
        self.folders.sort_by_cached_key(|f| natural(&f.name));
        self.count = self.presets.len();
        for f in &mut self.folders {
            f.settle();
            self.count += f.count;
        }
    }
}

/// A sort key reading runs of digits as numbers: "2 Legato" before "10 Shorts".
#[must_use]
pub fn natural(text: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let (number, tail) = rest.split_at(digits);
        let words = tail
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(tail.len());
        let (word, tail) = tail.split_at(words);
        out.push((number.parse().unwrap_or(0), word.to_lowercase()));
        rest = tail;
    }
    out
}
