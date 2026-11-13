//! The XML a program stores that nothing here reads, as `ignored …`
//! diagnostics. A name no source file of this module tree quotes as a
//! string is never read, whatever element it sits on, so the check is
//! conservative: it misses an attribute read for one element and dropped on
//! another, and never reports one that is read. Editor-only names are listed
//! as cosmetic and stay quiet.

use roxmltree::Node;

use kontra_format::Notes;

/// Every source file of `program` and its submodules.
const SOURCES: &[&str] = &[
    include_str!("../program.rs"),
    include_str!("blend.rs"),
    include_str!("delay.rs"),
    include_str!("digital_eq.rs"),
    include_str!("fade.rs"),
    include_str!("files.rs"),
    include_str!("filters.rs"),
    include_str!("gains.rs"),
    include_str!("lfo_routes.rs"),
    include_str!("load.rs"),
    include_str!("matrix.rs"),
    include_str!("modulation.rs"),
    include_str!("multi_peak.rs"),
    include_str!("provenance.rs"),
    include_str!("report.rs"),
    include_str!("reverb.rs"),
    include_str!("routing.rs"),
    include_str!("routing/inserts.rs"),
    include_str!("routing/moved.rs"),
    include_str!("routing/shaper.rs"),
    include_str!("routing/sweep.rs"),
    include_str!("tree.rs"),
    include_str!("values.rs"),
];

/// Names that only describe the editor: window layout, colours, notes.
const COSMETIC: &[&str] = &[
    "Name",
    "DisplayName",
    "Comment",
    "Color",
    "Colour",
    "Icon",
    "Image",
    "X",
    "Y",
    "Width",
    "Height",
    "Expanded",
    "Selected",
    "Collapsed",
];

/// Whether a source quotes `name`, or (an attribute built as `Type{n}`)
/// a literal starting with its name without trailing digits.
fn quoted(name: &str, indexed: bool) -> bool {
    let needle = match indexed {
        true => format!("\"{}", name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_')),
        false => format!("\"{name}\""),
    };
    SOURCES.iter().any(|s| s.contains(&needle))
}

/// Reports each element and attribute under `program` no source quotes.
pub(super) fn report(program: Node, notes: &mut Notes) {
    for node in program.descendants().filter(Node::is_element) {
        let tag = node.tag_name().name();
        // A container's children are checked on their own, and a quoted
        // container's children are read as a list.
        let listed = node.parent().is_some_and(|p| quoted(p.tag_name().name(), false));
        if !node.children().any(|c| c.is_element()) && !listed && !quoted(tag, false) {
            notes.ignored(&format!("element {tag}"), node.attribute("Name").unwrap_or(""));
        }
        for a in node.attributes() {
            let (name, value) = (a.name(), a.value().trim());
            if !(COSMETIC.contains(&name) || matches!(value, "" | "0") || quoted(name, true)) {
                notes.ignored(&format!("{tag}.{name}"), value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// A source file added to the module tree must join [`super::SOURCES`].
    #[test]
    fn every_source_file_is_listed() {
        fn files(dir: &std::path::Path, out: &mut usize) {
            for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    files(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    *out += 1;
                }
            }
        }
        let mut n = 0;
        files(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/program"), &mut n);
        // tests.rs and this file are not read for names.
        assert_eq!(n, super::SOURCES.len() - 1 + 2);
    }
}
