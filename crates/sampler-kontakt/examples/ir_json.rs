//! KONTRA's side of the native-oracle diff (tools/kontakt-oracle/diff.py):
//! translate one .nki with the production reader and print the IR as JSON.
//! Sample data and script source are not part of the IR, so nothing decrypted
//! is printed.
//!
//!     cargo run -p sampler-kontakt --example ir_json -- A.nki > a.json

fn main() {
    let path = std::env::args_os().nth(1).expect("usage: ir_json A.nki");
    let out = match sampler_kontakt::read(std::path::Path::new(&path)) {
        Ok(k) => serde_json::json!({
            "instrument": k.instrument,
            "locations": k.locations,
        }),
        Err(e) => serde_json::json!({ "load_error": e.to_string() }),
    };
    println!("{out}");
}
