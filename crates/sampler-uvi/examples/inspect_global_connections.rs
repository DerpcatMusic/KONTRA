use std::path::Path;
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (xml, _) = sampler_uvi::Bank::open(Path::new(&args[0]))?.program(&args[1])?;
    let doc = roxmltree::Document::parse(&xml)?;
    for owner in doc
        .descendants()
        .filter(|n| n.has_tag_name("Program") || n.has_tag_name("Layer"))
    {
        for c in owner
            .children()
            .filter(|n| n.has_tag_name("Connections"))
            .flat_map(|n| n.children())
            .filter(|n| {
                n.is_element() && matches!(n.attribute("Destination"), Some("Gain" | "Pan"))
            })
        {
            let source = c.attribute("Source").unwrap_or("");
            let (scope, name) = match source.split_once('/') {
                Some(("$Program", name)) => (Some("Program"), name),
                Some(("$Layer", name)) => (Some("Layer"), name),
                _ => (None, source),
            };
            let definition = owner
                .ancestors()
                .filter(|n| n.is_element() && scope.is_none_or(|s| n.has_tag_name(s)))
                .find_map(|n| {
                    n.children()
                        .filter(|n| n.has_tag_name("ControlSignalSources"))
                        .flat_map(|n| n.children())
                        .find(|n| n.is_element() && n.attribute("Name") == Some(name))
                });
            println!(
                "{} {:?} source={source:?} destination={:?} amount={:?} definition={:?}",
                owner.tag_name().name(),
                owner.attribute("Name"),
                c.attribute("Destination"),
                c.attribute("Amount"),
                definition.map(|n| (
                    n.tag_name().name(),
                    n.attributes()
                        .map(|a| (a.name(), a.value()))
                        .collect::<Vec<_>>()
                ))
            );
        }
    }
    Ok(())
}
