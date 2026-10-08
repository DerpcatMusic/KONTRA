fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("create-library") => kontakto::creator::create_library(&args[1..]),
        Some("--version" | "-V") => { println!("{}", kontakto::build_info::SUMMARY); Ok(()) }
        _ => { println!("kontakto create-library <samples folder> [--name NAME] [--vendor NAME] [--out DIR]"); Ok(()) }
    }
}
