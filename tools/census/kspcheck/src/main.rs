//! Compile every KSP file named on stdin; print `<ok|err>\t<path>\t<message>`.
//!
//! Every line becomes exactly one output line, including the ones that cannot
//! be used: a path line that is not UTF-8, a file that cannot be read, a file
//! that is not UTF-8. The callers count `ok` and `err` lines to decide how many
//! slots the corpus has, so a path dropped here makes the corpus quietly
//! smaller — and a smaller corpus reads as a better one. Strict decoding stays
//! strict: a script that is not valid UTF-8 is reported with the byte offset
//! rather than decoded lossily, because a lossily-decoded script is a different
//! program from the one on disk.
use std::io::{self, BufRead};

fn main() {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut buf = Vec::new();
    let mut line_no = 0usize;
    loop {
        buf.clear();
        match input.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => line_no += 1,
            Err(e) => {
                // The stream itself is gone: this is still a failure line, not
                // an early exit that looks like a short corpus.
                println!("err\t<line {}>\tstdin read error: {e}", line_no + 1);
                break;
            }
        }
        let path = match std::str::from_utf8(&buf) {
            Ok(s) => s.trim_end_matches(['\n', '\r']),
            Err(e) => {
                println!(
                    "err\t<line {}>\tstdin: not UTF-8 at byte {}",
                    line_no,
                    e.valid_up_to()
                );
                continue;
            }
        };
        if path.is_empty() {
            continue;
        }
        let src = match std::fs::read(path) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(src) => src,
                Err(e) => {
                    println!(
                        "err\t{path}\tnot UTF-8: invalid byte at offset {}",
                        e.utf8_error().valid_up_to()
                    );
                    continue;
                }
            },
            Err(e) => {
                println!("err\t{path}\tunreadable: {e}");
                continue;
            }
        };
        match kontra_ksp::compile(&src, &[]) {
            Ok(_) => println!("ok\t{path}\t"),
            Err(e) => println!("err\t{path}\t{}", e.to_string().replace('\n', " ")),
        }
    }
}
