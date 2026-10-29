//! What a library is called: its `.nicnt` product, else its folder's name.

use std::io::Read;
use std::path::Path;

/// The product's name and company from a `.nicnt`'s product XML, read from
/// its first 64 KiB. Only these two fields are read.
#[must_use]
pub fn product(nicnt: &Path) -> Option<(String, String)> {
    let mut bytes = Vec::new();
    std::fs::File::open(nicnt)
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let field = |tag: &str| -> Option<String> {
        let open = format!("<{tag}>");
        let start = text.find(&open)? + open.len();
        let rest = text.get(start..)?;
        let value = rest.get(..rest.find('<')?)?;
        let value = value
            .replace("&amp;", "&")
            .replace("&apos;", "'")
            .replace("&quot;", "\"")
            .replace("&lt;", "<")
            .replace("&gt;", ">");
        let value = value.trim();
        (!value.is_empty() && value.len() < 120).then(|| value.to_owned())
    };
    Some((field("Name")?, field("Company").unwrap_or_default()))
}

/// A folder name as a library name, and the vendor it names in square
/// brackets: "Areia 1.2.0 [Audio Imperia]" is Areia, by Audio Imperia.
#[must_use]
pub fn clean_name(folder: &str) -> (String, String) {
    let mut vendor = String::new();
    let mut kept = String::new();
    let (mut depth, mut inner, mut square) = (0, String::new(), false);
    for c in folder.chars() {
        match c {
            '[' | '(' | '{' => {
                if depth == 0 {
                    inner.clear();
                    square = c == '[';
                }
                depth += 1;
                kept.push(' ');
            }
            ']' | ')' | '}' if depth > 0 => {
                depth -= 1;
                let words = words(&inner);
                if depth == 0 && square && !words.is_empty() {
                    vendor = words.join(" ");
                }
            }
            _ if depth > 0 => inner.push(c),
            _ => kept.push(c),
        }
    }
    let name = words(&kept).join(" ");
    let name = name.trim_matches(|c: char| c == '-' || c == ' ');
    if name.is_empty() {
        return (folder.trim().to_owned(), vendor);
    }
    (name.to_owned(), vendor)
}

/// The words of a name without underscores, versions or packaging noise.
fn words(text: &str) -> Vec<&str> {
    let version = |w: &str| {
        let w = w.strip_prefix(['v', 'V']).unwrap_or(w);
        !w.is_empty()
            && w.starts_with(|c: char| c.is_ascii_digit())
            && (w.contains('.') || w.len() < 3 && text.contains(['v', 'V']))
            && w.chars()
                .all(|c| c.is_ascii_digit() || c == '.' || c.is_ascii_lowercase() && c < 'g')
    };
    let noise = |w: &str| {
        matches!(
            w.to_lowercase().as_str(),
            "kontakt" | "library" | "nki" | "full" | "win" | "mac" | "osx" | "pc" | "update"
        )
    };
    text.split(|c: char| c.is_whitespace() || c == '_')
        .filter(|w| !w.is_empty() && !version(w) && !noise(w))
        .collect()
}
