//! Writes an instrument's script sources and saved values to a folder,
//! one pair of files per slot, and the bank modules its scripts may load:
//! `cargo run -p kontra-format-uvi --example scripts -- <file> <dir>`

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let (Some(file), Some(dir)) = (args.next(), args.next()) else {
        return Err("usage: scripts <file> <dir>".into());
    };
    let i = kontra_format_uvi::load(std::path::Path::new(&file))?;
    std::fs::create_dir_all(&dir)?;
    for s in &i.scripts {
        let out =
            std::path::Path::new(&dir).join(format!("{} {}.lua", s.slot, s.name.replace('/', "_")));
        std::fs::write(&out, &s.source)?;
        std::fs::write(out.with_extension("saved"), s.saved.join("\n"))?;
        // The modules `require` may load, under their bank paths.
        for (path, bytes) in &s.data {
            let to = std::path::Path::new(&dir)
                .join("modules")
                .join(path.trim_start_matches('/'));
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(to, bytes)?;
        }
        println!("{}", out.display());
    }
    Ok(())
}
