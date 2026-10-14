use sampler_ui_ir::{MenuHierarchyError as Error, MenuItem, menu_hierarchy};

fn items(paths: &[&str]) -> Vec<MenuItem> {
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| MenuItem {
            text: (*path).into(),
            value: 10 + index as i32 * 7,
            visible: true,
        })
        .collect()
}

#[test]
fn path_groups_keep_authored_order_unicode_and_original_leaf_indices() {
    let mut source = items(&["a/hidden", "a/first", "b/middle", "a/深い/last", "plain"]);
    source[0].visible = false;
    let tree = menu_hierarchy(&source).unwrap();
    assert_eq!(
        tree.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(),
        ["a", "b", "plain"]
    );
    assert_eq!(tree[0].children[0].item_index, Some(1));
    assert_eq!(tree[0].children[1].label, "深い");
    assert_eq!(tree[0].children[1].children[0].item_index, Some(3));
    assert_eq!(tree[1].children[0].item_index, Some(2));
    assert_eq!(tree[2].item_index, Some(4));
}

#[test]
fn ambiguous_paths_and_renderer_limits_are_explicit() {
    for paths in [["a", "a/b"], ["a/b", "a"]] {
        assert_eq!(menu_hierarchy(&items(&paths)), Err(Error::LeafIsAlsoBranch));
    }
    assert_eq!(
        menu_hierarchy(&items(&["a/b", "a/b"])),
        Err(Error::DuplicatePath)
    );
    for path in ["", "/a", "a/", "a//b"] {
        assert_eq!(menu_hierarchy(&items(&[path])), Err(Error::EmptySegment));
    }
    let deep = vec!["a"; 33].join("/");
    assert_eq!(menu_hierarchy(&items(&[&deep])), Err(Error::RendererLimit));
    let wide = (0..16_385)
        .map(|n| MenuItem {
            text: n.to_string(),
            value: n,
            visible: true,
        })
        .collect::<Vec<_>>();
    assert_eq!(menu_hierarchy(&wide), Err(Error::RendererLimit));
}
