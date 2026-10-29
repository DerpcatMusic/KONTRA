//! `--flag value` parsing. Std only: xtask takes no CLI crate.

/// The value that follows `name`, if the flag is given.
pub fn value(args: &[String], name: &str) -> Result<Option<String>, String> {
    match args.iter().position(|arg| arg == name) {
        None => Ok(None),
        Some(at) => match args.get(at + 1) {
            Some(value) => Ok(Some(value.clone())),
            None => Err(format!("{name} needs a value")),
        },
    }
}

/// Whether a value-less flag is given.
pub fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

/// The numeric value of `name`, or `default` when the flag is absent.
pub fn number(args: &[String], name: &str, default: usize) -> Result<usize, String> {
    match value(args, name)? {
        None => Ok(default),
        Some(text) => text
            .parse()
            .map_err(|_| format!("{name} expects a number, got `{text}`")),
    }
}

/// Rejects any `--option` the command does not know, so a typo cannot quietly change a gate.
pub fn reject_unknown(args: &[String], known: &[&str]) -> Result<(), String> {
    match args
        .iter()
        .find(|arg| arg.starts_with("--") && !known.contains(&arg.as_str()))
    {
        Some(arg) => Err(format!("unknown option `{arg}`")),
        None => Ok(()),
    }
}
