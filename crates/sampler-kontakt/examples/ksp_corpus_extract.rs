//! Write each instrument's KSP source (inline or linked `.txt`) from Kontakt `.nki`
//! files, for the Luau equivalence corpus. Same chunk walk as the library loader.
//! Usage: ksp_corpus_extract <out-dir> <file.nki>...
//! Writes `<out-dir>/<instrument>-<slot>.ksp`; bypassed and empty slots are skipped.
use ni_file::kontakt::objects::{BParScript, Program};
use std::path::{Path, PathBuf};

const PROGRAM: u16 = 0x28;
const SCRIPT: u16 = 6;

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().expect("usage: ksp_corpus_extract <out-dir> <file.nki>..."));
    std::fs::create_dir_all(&out).expect("create out dir");
    let (mut written, mut skipped) = (0usize, 0usize);
    for path in args {
        let path = Path::new(&path);
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().replace(['/', ' ', '[', ']'], "_");
        let Ok(chunks) = sampler_kontakt::read_chunks(path) else {
            skipped += 1;
            continue;
        };
        let Some(program) = chunks.find_first(PROGRAM).and_then(|c| Program::try_from(c).ok()) else {
            skipped += 1;
            continue;
        };
        let mut resources = None;
        for (slot, chunk) in program.0.children.iter().filter(|c| c.id == SCRIPT).enumerate() {
            let Ok(script) = BParScript::try_from(chunk).and_then(|s| s.params()) else {
                continue;
            };
            if script.bypass {
                continue;
            }
            let mut text = script.text.clone();
            if let Some(link) = script.textfile_name.as_deref().filter(|n| !n.trim().is_empty())
                && let Some(linked) = resources.get_or_insert_with(|| sampler_kontakt::Resources::of(path)).script(link)
            {
                text = Some(linked);
            }
            let Some(text) = text.filter(|t| !t.trim().is_empty()) else {
                continue;
            };
            let target = out.join(format!("{stem}-{slot}.ksp"));
            std::fs::write(&target, text).expect("write script");
            written += 1;
        }
    }
    println!("written {written} scripts, skipped {skipped} containers");
}
