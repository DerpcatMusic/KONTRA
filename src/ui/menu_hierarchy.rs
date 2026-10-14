//! Validated Falcon path menus through the same popup/input/paint facilities.
use super::*;
use std::hash::{Hash, Hasher};

fn category_id(anchor: &str, path: &[String], label: &str) -> String {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hash);
    label.hash(&mut hash);
    format!("{anchor}-category-{:x}", hash.finish())
}

fn entry_id(anchor: &str, path: &[String], node: &ir::MenuNode) -> String {
    node.item_index.map_or_else(
        || category_id(anchor, path, &node.label),
        |at| format!("{anchor}-item-{at}"),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn popup(
    ui: &mut Ui,
    namespace: &str,
    face: &Interface,
    n: WidgetRef,
    tree: &[ir::MenuNode],
    scale: f64,
    values: &mut Values,
    input: &mut InputState,
    width: f64,
    height: f64,
) -> Option<El> {
    let wd = face.widgets.get(n.0)?;
    let Kind::Menu { items } = &wd.kind else {
        return None;
    };
    let anchor = target(namespace, n);
    let popup = format!("{anchor}-popup");
    if tree.is_empty() {
        input.menu = None;
        input.menu_path.clear();
        input.menu_hovered = None;
        return None;
    }
    let root = if namespace.is_empty() {
        "ir-view".into()
    } else {
        format!("{namespace}-ir-view")
    };
    let origin = ui
        .scene()
        .and_then(|scene| scene.surface(&root))
        .map(|s| s.frame);
    let source = face.page_rect(n);
    let (ax, ay, aw, ah) = ui
        .scene()
        .and_then(|scene| scene.surface(&anchor))
        .map(|s| {
            (
                s.frame.x - origin.map_or(0., |r| r.x),
                s.frame.y - origin.map_or(0., |r| r.y),
                s.frame.size.width,
                s.frame.size.height,
            )
        })
        .unwrap_or((
            f64::from(source.x) * scale,
            f64::from(source.y) * scale,
            f64::from(source.width) * scale,
            f64::from(source.height) * scale,
        ));
    let w = aw.max(120. * scale).min(width);
    let row_h = CONTROL * scale;
    let root_h = (tree.len() as f64 * row_h).min(height);
    let root_x = ax.clamp(0., (width - w).max(0.));
    let root_y = (ay + ah).clamp(0., (height - root_h).max(0.));
    let (mut x, mut y) = (root_x, root_y);
    let mut prefix = Vec::<String>::new();
    let mut nodes = tree;
    let mut parent_id = None::<String>;
    let mut columns = Vec::new();
    let mut hovered = None;
    for depth in 0..32 {
        let mut rows = Vec::new();
        for (index, node) in nodes.iter().enumerate() {
            let id = entry_id(&anchor, &prefix, node);
            let (is_hovered, activated) = {
                let response = ui.get(id.as_str());
                (response.hovered, response.activated())
            };
            if is_hovered {
                hovered = Some(id.clone());
            }
            let entered = is_hovered && input.menu_hovered.as_ref() != Some(&id);
            let right = ui.keys(id.as_str()).iter().any(|key| key.key == Key::Right);
            let left = ui.keys(id.as_str()).iter().any(|key| key.key == Key::Left);
            let next = ui.keys(id.as_str()).iter().find_map(|key| match key.key {
                Key::Down => Some((index + 1).min(nodes.len() - 1)),
                Key::Up => Some(index.saturating_sub(1)),
                _ => None,
            });
            if let Some(next) = next {
                ui.focus(entry_id(&anchor, &prefix, &nodes[next]));
            }
            if left && depth > 0 {
                input.menu_path.truncate(depth - 1);
                if let Some(parent) = &parent_id {
                    ui.focus(parent);
                }
            }
            if let Some(at) = node.item_index {
                if entered {
                    input.menu_path.truncate(depth);
                }
                if activated && let Some(item) = items.get(at).filter(|item| item.visible) {
                    if let Binding::Control(control) = wd.binding {
                        values.insert(control, f64::from(item.value));
                        input.edits.push(Edit {
                            widget: n,
                            index: 0,
                            value: ir::Value::Integer(item.value),
                            mods: Mods::default(),
                            mouse_over: false,
                            cursor: 0,
                            event: 0,
                        });
                    }
                    input.menu = None;
                    input.menu_path.clear();
                    input.menu_hovered = None;
                    return None;
                }
                rows.push(
                    caption(node.label.clone())
                        .pad((TIGHT * scale, SPACE * scale))
                        .h(row_h)
                        .fill(Role::Ink)
                        .focusable()
                        .a11y(A11y::Button)
                        .named(node.label.clone())
                        .id(id),
                );
            } else {
                if entered || activated || right {
                    input.menu_path.truncate(depth);
                    input.menu_path.push(node.label.clone());
                    if right && let Some(first) = node.children.first() {
                        let mut path = prefix.clone();
                        path.push(node.label.clone());
                        ui.focus(entry_id(&anchor, &path, first));
                    }
                }
                let open = input.menu_path.get(depth) == Some(&node.label);
                rows.push(
                    row![
                        caption(node.label.clone()).fill(Role::Ink).flex(1),
                        glyph(Icon::Right, TEXT * scale, secondary())
                    ]
                    .align(Align::Center)
                    .pad((TIGHT * scale, SPACE * scale))
                    .h(row_h)
                    .fill(if open { Role::Raised } else { Role::Field })
                    .focusable()
                    .a11y(A11y::Button)
                    .named(format!("Open {}", node.label))
                    .id(id),
                );
            }
        }
        let column_h = (rows.len() as f64 * row_h).min(height);
        columns.push(
            col(rows)
                .gap(0)
                .w(w)
                .h(column_h)
                .scroll()
                .fill(Role::Field)
                .stroke(Role::Ink.alpha(0.3))
                .stroke_width(1)
                .id(format!("{popup}-column-{depth}"))
                .at(x - root_x, y - root_y),
        );
        let Some(label) = input.menu_path.get(depth) else {
            break;
        };
        let Some((index, branch)) = nodes
            .iter()
            .enumerate()
            .find(|(_, node)| node.item_index.is_none() && &node.label == label)
        else {
            input.menu_path.truncate(depth);
            break;
        };
        let id = category_id(&anchor, &prefix, &branch.label);
        let row_y = ui
            .scene()
            .and_then(|scene| scene.surface(&id))
            .map_or(y + index as f64 * row_h, |s| {
                s.frame.y - origin.map_or(0., |r| r.y)
            });
        let child_h = (branch.children.len() as f64 * row_h).min(height);
        x = if x + w + w <= width {
            x + w
        } else {
            (x - w).max(0.)
        };
        y = row_y.clamp(0., (height - child_h).max(0.));
        prefix.push(branch.label.clone());
        nodes = &branch.children;
        parent_id = Some(id);
    }
    ui.capture_popup_wheel(popup.clone());
    input.menu_hovered = hovered;
    Some(stack(columns).w(w).h(root_h).id(popup).at(root_x, root_y))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        host: sampler_uvi::script::ScriptHost,
        face: Interface,
        ui: Ui,
        values: Values,
        input: InputState,
    }
    impl Fixture {
        fn new() -> Self {
            let xml = "<UVI4><Program><EventProcessors><ScriptProcessor><script><![CDATA[setSize(320,200) local m=Menu('Routes',{'A/First','B/Middle','A/Last'},1) m.bounds={10,10,100,25} m.hierarchical=true m.changed=function(self) picked=self.value pickedText=self:getText(self.value) end]]></script></ScriptProcessor></EventProcessors></Program></UVI4>";
            let host = sampler_uvi::script::ScriptHost::new(
                xml,
                (),
                sampler_uvi::script::Config::default(),
            )
            .unwrap();
            let face = host.interface();
            assert!(face.unsupported.is_empty(), "{:?}", face.unsupported);
            let values = host.control_values().into_iter().collect();
            let mut f = Self {
                host,
                face,
                ui: crate::ui::theme::ui(),
                values,
                input: InputState::default(),
            };
            for _ in 0..3 {
                f.tick(Input::default());
            }
            f
        }
        fn tick(&mut self, event: Input) {
            let root = view_state(
                &mut self.ui,
                "tree",
                &self.face,
                PageRef(0),
                &Assets::default(),
                Presentation::Bitmap,
                1.,
                &mut self.values,
                &mut self.input,
            );
            self.ui
                .frame(root, Some(Size::new(320., 200.)), event, 1. / 60.)
                .unwrap();
        }
        fn click(&mut self, id: &str) {
            let frame = self
                .ui
                .scene()
                .unwrap()
                .surface(id)
                .unwrap_or_else(|| panic!("missing target {id}"))
                .frame;
            let pos = Some(Point::new(
                frame.x + frame.size.width / 2.,
                frame.y + frame.size.height / 2.,
            ));
            for down in [true, false] {
                self.tick(Input {
                    pointer: PointerInput {
                        pos,
                        buttons: if down {
                            Buttons::PRIMARY
                        } else {
                            Buttons::default()
                        },
                        ..Default::default()
                    },
                    ..Default::default()
                });
            }
            for _ in 0..2 {
                self.tick(Input::default());
            }
        }
        fn key(&mut self, key: Key) {
            self.tick(Input {
                keys: vec![KeyPress {
                    key,
                    mods: Mods::default(),
                }],
                ..Default::default()
            });
            self.tick(Input::default());
        }
    }

    #[test]
    fn flyout_leaf_outside_parent_popup_selects_its_original_lua_index() {
        let mut f = Fixture::new();
        let before = crate::ui::tests::pixels(&f.ui, 320, 200);
        f.click("tree-ir-0");
        f.click(&category_id("tree-ir-0", &[], "A"));
        assert!(
            f.input.edits.is_empty(),
            "opening a category never changes its value"
        );
        let scene = f.ui.scene().unwrap();
        let root = scene.surface("tree-ir-0-popup").unwrap().frame;
        let leaf = scene.surface("tree-ir-0-item-2").unwrap().frame;
        assert!(
            leaf.x >= root.x + root.size.width,
            "child really escapes the parent's popup frame"
        );
        let painted = crate::ui::tests::pixels(&f.ui, 320, 200);
        let child_pixels = |image: &[u8]| {
            (leaf.y.ceil() as usize..(leaf.y + leaf.size.height).floor() as usize)
                .flat_map(|y| {
                    let start = (y * 320 + leaf.x.ceil() as usize) * 4;
                    let end = (y * 320 + (leaf.x + leaf.size.width).floor() as usize) * 4;
                    image[start..end].iter().copied()
                })
                .collect::<Vec<_>>()
        };
        assert_ne!(
            child_pixels(&painted),
            child_pixels(&before),
            "escaped flyout rows are painted"
        );
        f.click("tree-ir-0-item-2");
        let Binding::Control(control) = f.face.widgets[0].binding else {
            panic!("control missing")
        };
        assert_eq!(f.values[&control], 3., "grouping must not renumber a leaf");
        assert!(
            f.input
                .edits
                .iter()
                .any(|edit| edit.value == ir::Value::Integer(3))
        );
        f.host.set_control(control, f.values[&control]).unwrap();
        assert_eq!(f.host.global_text("picked"), "3");
        assert_eq!(f.host.global_text("pickedText"), "A/Last");
        assert!(f.input.menu.is_none());
        assert!(f.ui.scene().unwrap().surface("tree-ir-0-popup").is_none());
    }

    #[test]
    fn keyboard_opens_child_returns_to_parent_and_escape_dismisses() {
        let mut f = Fixture::new();
        f.ui.focus("tree-ir-0");
        f.key(Key::Enter);
        let category = category_id("tree-ir-0", &[], "A");
        f.ui.focus(&category);
        f.key(Key::Right);
        assert!(f.ui.scene().unwrap().surface("tree-ir-0-item-0").is_some());
        assert!(f.ui.focused("tree-ir-0-item-0"));
        f.key(Key::Left);
        assert!(f.input.menu_path.is_empty());
        assert!(f.ui.focused(&category));
        f.key(Key::Escape);
        assert!(f.input.menu.is_none());
        assert!(f.ui.scene().unwrap().surface("tree-ir-0-popup").is_none());
    }

    #[test]
    fn keyboard_selection_keeps_original_index_and_right_edge_flyout_is_reachable() {
        let mut f = Fixture::new();
        f.face.widgets[0].rect.x = 210.;
        for _ in 0..3 {
            f.tick(Input::default());
        }
        f.ui.focus("tree-ir-0");
        f.key(Key::Enter);
        let category = category_id("tree-ir-0", &[], "A");
        f.ui.focus(&category);
        f.key(Key::Right);
        let scene = f.ui.scene().unwrap();
        let root = scene.surface("tree-ir-0-popup").unwrap().frame;
        let first = scene.surface("tree-ir-0-item-0").unwrap().frame;
        assert!(first.x < root.x, "right-edge children open left");
        assert!(first.x >= 0. && first.x + first.size.width <= 320.);
        f.key(Key::Down);
        assert!(f.ui.focused("tree-ir-0-item-2"));
        f.key(Key::Enter);
        let Binding::Control(control) = f.face.widgets[0].binding else {
            panic!("control missing")
        };
        assert_eq!(f.values[&control], 3.);
        f.host.set_control(control, f.values[&control]).unwrap();
        assert_eq!(f.host.global_text("pickedText"), "A/Last");
        assert!(f.input.menu.is_none());
    }
}
