//! Opt-in provenance for XML translator applications.
use roxmltree::Node;

pub(super) fn record(
    id: &'static str,
    node: Node,
    extra: &impl std::fmt::Debug,
    output: &impl std::fmt::Debug,
) {
    kontra_law::record(|| kontra_law::Application {
        id: kontra_law::LawId(id),
        location: format!("xml/{}/{}", node.range().start, node.tag_name().name()),
        input: format!(
            "attributes={:?}, context={extra:?}",
            node.attributes()
                .map(|a| (a.name(), a.value()))
                .collect::<Vec<_>>()
        ),
        output: format!("{output:?}"),
    });
}
