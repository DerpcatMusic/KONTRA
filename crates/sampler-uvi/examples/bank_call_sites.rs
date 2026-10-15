//! Read-only structural Lua call sites. Prints identifiers and line numbers,
//! never script bodies, literals, sample payloads, or images.
use std::path::Path;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    assert!(args.len()==3||args.len()==4,"BANK SYMBOL [PROGRAM]");
    let bank = sampler_uvi::Bank::open(Path::new(&args[1]))?;
    let sources = bank.scripts().sources();
    let mut embedded=Vec::new();
    if let Some(member)=args.get(3) {
        let(text,_)=bank.program(member)?;
        let doc=roxmltree::Document::parse(&text)?;
        for (i,node) in doc.descendants().filter(|n|n.has_tag_name("script")).enumerate() {
            embedded.push((format!("embedded_{i}"),node.text().unwrap_or("").to_owned()));
        }
    }
    let mut rows = Vec::new();
    for (module, (path, source)) in sources.iter().chain(embedded.iter()).enumerate() {
        let mut enclosing = None;
        for (line, text) in source.lines().enumerate() {
            let trimmed = text.trim_start();
            if trimmed.starts_with("function ") || trimmed.starts_with("local function ") {
                let name = trimmed
                    .split_once("function ")
                    .unwrap()
                    .1
                    .split('(')
                    .next()
                    .unwrap()
                    .trim();
                enclosing = (!name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "_.:".contains(c)))
                .then(|| name.to_owned());
            }
            if text.contains(&args[2]) {
                rows.push(serde_json::json!({"module_index":module,"module_basename":path.rsplit('/').next(),"line":line+1,"nearest_preceding_named_function":enclosing}));
            }
        }
    }
    println!(
        "{}",
        serde_json::json!({"module_count":sources.len(),"matches":rows,"script_body_written":false})
    );
    Ok(())
}
