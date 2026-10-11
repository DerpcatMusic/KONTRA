fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("parity") => kontakto::parity::run(&args[1..]),
        Some("create-library") => kontakto::creator::create_library(&args[1..]),
        #[cfg(feature = "plugin")]
        Some("export-multi-state") => {
            anyhow::ensure!(
                args.len() == 3,
                "export-multi-state <v2.kontra-multi> <new.state>"
            );
            kontakto::export_multi_state(
                std::path::Path::new(&args[1]),
                std::path::Path::new(&args[2]),
            )
        }
        Some("--version" | "-V") => {
            println!("{}", kontakto::build_info::SUMMARY);
            Ok(())
        }
        _ => {
            println!(
                "kontakto create-library <samples folder> [--name NAME] [--vendor NAME] [--out DIR]\nkontakto parity <patch> [--json] [--lowered]\nkontakto parity --corpus <dir|items.tsv> [--json]"
            );
            Ok(())
        }
    }
}
