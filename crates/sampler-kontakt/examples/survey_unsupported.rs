//! Print every unsupported/approximate item of each instrument listed in
//! LISTFILE (one path per line; `.nkm` paths are expanded to every program)
//! after a full streamed load (RAM only). Usage: survey_unsupported LISTFILE
use sampler_kontakt::{Options, StreamPolicy, load_read_streamed};
use std::path::Path;

fn main() {
    let list = std::env::args()
        .nth(1)
        .map(|f| std::fs::read_to_string(f).unwrap())
        .unwrap_or_default();
    for path in list.lines() {
        let path = Path::new(path);
        let multi = path.extension().is_some_and(|e| e == "nkm");
        let programs = if multi {
            sampler_kontakt::read_multi(path).map_or(0, |m| m.programs.len())
        } else {
            1
        };
        for index in 0..programs {
            println!("== {} #{index}", path.display());
            let read = if multi {
                sampler_kontakt::read_program(path, index)
            } else {
                sampler_kontakt::read(path)
            };
            let options = Options {
                library: Some(path.into()),
                ..Options::default()
            };
            match read.and_then(|k| {
                load_read_streamed(k, &options, &StreamPolicy::default(), |_| {})
            }) {
                Ok(s) => {
                    for u in &s.loaded.instrument.unsupported {
                        println!("U\t{}\t{}\t{}\t{:?}", u.feature, u.value, u.location, u.reason);
                    }
                }
                Err(e) => println!("ERR {e:?}"),
            }
        }
    }
}
