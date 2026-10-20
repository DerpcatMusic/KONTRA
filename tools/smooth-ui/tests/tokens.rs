//! The rules the token layer exists to keep, checked against the source.
//!
//! These read the editor's own files rather than its rendered output, because
//! the rules are about where a value is allowed to live: a length may be
//! stated in `tokens`, a colour in `theme`, and nowhere else.
//!
//! A layout call takes a token, or arithmetic over tokens, or a value the
//! caller supplied -- never a bare number. A mark is drawn from the icon
//! pack, never from path data typed into a view. And a part reads a colour
//! from the active palette, so swapping the palette swaps the look.

use std::fs;

/// The files that compose rather than define.
const COMPOSED: [&str; 5] = [
    "src/view.rs",
    "src/skin.rs",
    "src/graph/draw.rs",
    "src/graph/mod.rs",
    "src/graph/band.rs",
];

/// The layout calls that place or size something.
const LAYOUT: [&str; 6] = [".gap(", ".pad(", ".size(", ".h(", ".w(", ".radius("];

fn source(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// The argument text of a call, from just past its `(` to the matching `)`.
fn argument(text: &str, open: usize) -> &str {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    for (index, byte) in bytes.iter().enumerate().skip(open) {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open + 1..index];
                }
            }
            _ => {}
        }
    }
    &text[open + 1..]
}

/// Whether an expression states a length outright.
///
/// A token path, a local, or arithmetic over either names something. A bare
/// number, or a tuple of bare numbers, does not: it is a length that escaped
/// the scales.
fn bare(expression: &str) -> bool {
    !expression
        .chars()
        .any(|c| c.is_ascii_alphabetic() || c == '_')
}

#[test]
fn layout_never_states_a_length() {
    let mut offenders = Vec::new();
    for name in COMPOSED {
        let text = source(name);
        for call in LAYOUT {
            let mut from = 0;
            while let Some(found) = text[from..].find(call) {
                let open = from + found + call.len() - 1;
                let expression = argument(&text, open);
                if bare(expression) {
                    let line = text[..open].lines().count();
                    offenders.push(format!("{name}:{line} {call}{expression})"));
                }
                from = open + 1;
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a layout call states a bare length; read a token instead:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn marks_come_from_the_icon_pack() {
    let mut offenders = Vec::new();
    for name in COMPOSED {
        for (line, text) in source(name).lines().enumerate() {
            // An SVG path starts with a move; the pack holds every one of
            // them, and a view may not type its own.
            if text.contains("\"M") && !text.trim_start().starts_with("//") {
                offenders.push(format!("{name}:{} {}", line + 1, text.trim()));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "path data outside the icon pack:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn colours_come_from_the_palette() {
    let mut offenders = Vec::new();
    for name in COMPOSED {
        for (line, text) in source(name).lines().enumerate() {
            let code = text.split("//").next().unwrap_or_default();
            if let Some(at) = code.find("0x") {
                // A hex run of six digits is a colour; four or eight would be
                // a mask or an alpha, and neither belongs here either, but a
                // six-digit run is the one a palette is for.
                let digits: String = code[at + 2..]
                    .chars()
                    .take_while(|c| c.is_ascii_hexdigit())
                    .collect();
                if digits.len() >= 6 {
                    offenders.push(format!("{name}:{} {}", line + 1, text.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a colour is stated outside the palette; read a theme token instead:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn every_theme_is_distinct_and_selectable() {
    use smooth_ui::theme;

    let mut ids = Vec::new();
    for entry in theme::THEMES {
        assert!(!entry.id.is_empty(), "a theme with no id");
        assert!(!entry.name.is_empty(), "a theme with no name: {}", entry.id);
        assert!(
            !ids.contains(&entry.id),
            "two themes share the id {:?}",
            entry.id
        );
        ids.push(entry.id);
    }

    // Selecting a name the product does not ship leaves the look alone
    // rather than leaving it undefined: a stale environment variable should
    // not stop music.
    let before = theme::active().id;
    assert!(theme::set("no-such-theme").is_none());
    assert_eq!(theme::active().id, before);

    for entry in theme::THEMES {
        let selected = theme::set(entry.id).expect("a shipped theme");
        assert_eq!(selected.id, entry.id);
        assert_eq!(theme::active().id, entry.id);
        assert_eq!(theme::t().ink, entry.palette.ink);
    }
    theme::set("graphite").expect("the default");
}
