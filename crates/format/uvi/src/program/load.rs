//! What a program's script does as it loads, carried into the model. A UVI
//! program may hold template samples that its script replaces when it loads
//! (`loadSample` from `onLoad`); the caller runs the script once and hands
//! back the samples it loaded, which go on their oscillators' zones here.

use std::collections::HashMap;
use std::path::PathBuf;

use kontra_format::Notes;
use kontra_ir as ir;

use super::{Library, lookup, sample_entry};

/// A sample a script puts on an oscillator as the program loads: the engine
/// group of the oscillator's keygroup, the oscillator's slot there, the path
/// the script names (relative to the script) and the base note it set.
#[derive(Clone, Debug, PartialEq)]
pub struct Swap {
    pub group: u32,
    pub slot: u16,
    pub path: String,
    pub root: Option<u8>,
}

/// A program translated but for its scripts' load pass: the model and what
/// the pass needs to put samples on it, without the program's document (a
/// big program's DOM would otherwise stay resident while the pass builds a
/// whole script state).
pub(crate) struct Unfinished<'a, L> {
    pub(super) program_dir: &'a str,
    pub(super) library: &'a L,
    pub(super) out: ir::Instrument,
    pub(super) notes: Notes,
    pub(super) samples: HashMap<(PathBuf, u64), usize>,
    pub(super) lookups: HashMap<String, Result<ir::Sample, String>>,
    pub(super) script_dirs: Vec<String>,
}

impl<L: Library> Unfinished<'_, L> {
    /// Runs each script's load through `swaps`, puts the samples it loads on
    /// the zones of their oscillators, and closes the diagnostics.
    pub(crate) fn finish(mut self, swaps: &dyn Fn(&ir::Script) -> Vec<Swap>) -> ir::Instrument {
        self.swap_samples(swaps);
        self.out.diagnostics = self.notes.finish();
        self.out
    }

    fn swap_samples(&mut self, swaps: &dyn Fn(&ir::Script) -> Vec<Swap>) {
        let runs: Vec<(String, Vec<Swap>)> = self
            .out
            .scripts
            .iter()
            .zip(&self.script_dirs)
            .map(|(script, dir)| (dir.clone(), swaps(script)))
            .collect();
        for (dir, list) in runs {
            for swap in list {
                let path = format!("{dir}/{}", swap.path);
                let sample = match lookup(&mut self.lookups, self.library, self.program_dir, &path)
                {
                    Ok(sample) => sample,
                    Err(e) => {
                        self.notes.add(ir::Impact::Broken, "script sample", e);
                        continue;
                    }
                };
                let frames = sample.frames;
                let index = sample_entry(&mut self.samples, &mut self.out.samples, sample);
                let Some(group) = self.out.groups.get_mut(swap.group as usize) else {
                    continue;
                };
                for zone in group.zones.iter_mut().filter(|z| z.slot == swap.slot) {
                    zone.sample = index;
                    zone.start = 0;
                    zone.end = frames;
                    zone.loops.clear();
                    zone.root = swap.root.unwrap_or(zone.root);
                }
            }
        }
    }
}

/// The JSON a script saved (`onSave`) as a Lua module returning the same
/// value: arrays and objects become tables, `null` nil.
pub(super) fn lua_literal(json: &str) -> String {
    let mut out = String::from("return ");
    let mut chars = json.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                let mut text = String::from('"');
                while let Some(c) = chars.next() {
                    match c {
                        '\\' if chars.peek() == Some(&'u') => {
                            chars.next();
                            let hex: String = chars.by_ref().take(4).collect();
                            text.push_str(&format!("\\u{{{hex}}}"));
                        }
                        // JSON's `\/` is not a Lua escape.
                        '\\' if chars.peek() == Some(&'/') => {
                            text.extend(chars.next());
                        }
                        '\\' => {
                            text.push(c);
                            text.extend(chars.next());
                        }
                        '"' => break,
                        _ => text.push(c),
                    }
                }
                text.push('"');
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                // A key: `"name":` becomes `["name"]=`.
                if chars.peek() == Some(&':') {
                    chars.next();
                    out.push_str(&format!("[{text}]="));
                } else {
                    out.push_str(&text);
                }
            }
            '[' => out.push('{'),
            ']' => out.push('}'),
            // Outside strings an `n` starts `null`.
            'n' => {
                chars.by_ref().take(3).for_each(drop);
                out.push_str("nil");
            }
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::lua_literal;

    #[test]
    fn saved_json_becomes_a_lua_table() {
        assert_eq!(
            lua_literal(r#"[{"loadedLayer":"A\/B \"x\""},{"n":1.5,"u":"\u00e9"},null]"#),
            r#"return {{["loadedLayer"]="A/B \"x\""},{["n"]=1.5,["u"]="\u{00e9}"},nil}"#
        );
    }
}
