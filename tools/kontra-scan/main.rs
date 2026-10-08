fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().is_some_and(|a| a == "--worker") {
        std::panic::set_hook(Box::new(|_| {}));
        let id = args.get(1).expect("worker id").clone();
        let out = std::path::PathBuf::from(args.get(2).expect("worker cache"));
        let result = std::thread::Builder::new()
            .stack_size(32 << 20)
            .spawn(move || kontakto::scan_one(&id, &out))?
            .join();
        let result = result.unwrap_or_else(|_| serde_json::json!({"loads":"no", "ui":"error", "plays_note":"no", "reason":"worker panic"}));
        println!("{}", serde_json::to_string(&result)?);
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    let driver = std::env::var_os("KONTRA_SCAN_DRIVER")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| exe.parent().unwrap().join("kontra_scan.py"));
    let status = std::process::Command::new("python3")
        .arg(driver)
        .arg("--engine")
        .arg(exe)
        .args(args)
        .status()?;
    std::process::exit(status.code().unwrap_or(1));
}
