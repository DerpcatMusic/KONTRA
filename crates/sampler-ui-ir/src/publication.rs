//! Sparse cumulative changes relative to a generation's immutable authored interface.
use super::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InterfacePatch {
    pub source: Option<Source>,
    pub performance: Option<bool>,
    pub pages: Option<Vec<Page>>,
    pub widgets: Vec<(usize, Widget)>,
    pub paint_order: Option<Option<Vec<WidgetRef>>>,
    pub widget_count: Option<usize>,
    pub assets: Option<Vec<Asset>>,
    pub styles: Option<Vec<TextStyle>>,
    pub icon: Option<(Option<AssetRef>, bool)>,
    pub native_ui: Option<Option<NativeUi>>,
    pub unsupported: Option<Vec<Unsupported>>,
}

impl InterfacePatch {
    pub fn between(base: &Interface, current: &Interface) -> Self {
        Self {
            source: (base.source != current.source).then_some(current.source),
            performance: (base.performance != current.performance).then_some(current.performance),
            pages: (base.pages != current.pages).then(|| current.pages.clone()),
            paint_order: (base.paint_order != current.paint_order).then(|| current.paint_order.clone()),
            widgets: current
                .widgets
                .iter()
                .enumerate()
                .filter(|(n, w)| base.widgets.get(*n) != Some(*w))
                .map(|(n, w)| (n, w.clone()))
                .collect(),
            widget_count: (base.widgets.len() != current.widgets.len())
                .then_some(current.widgets.len()),
            assets: (base.assets != current.assets).then(|| current.assets.clone()),
            styles: (base.styles != current.styles).then(|| current.styles.clone()),
            icon: ((base.icon, base.icon_hidden) != (current.icon, current.icon_hidden))
                .then_some((current.icon, current.icon_hidden)),
            native_ui: (base.native_ui != current.native_ui).then(||current.native_ui.clone()),
            unsupported: (base.unsupported != current.unsupported)
                .then(|| current.unsupported.clone()),
        }
    }

    /// Update an existing view without discarding its widget/asset identities.
    /// `previous` lets a field reverting to its authored value remove its override.
    pub fn apply(&self, base: &Interface, previous: &Self, view: &mut Interface) {
        if self.source != previous.source {
            view.source = self.source.unwrap_or(base.source);
        }
        if self.performance != previous.performance {
            view.performance = self.performance.unwrap_or(base.performance);
        }
        if self.pages != previous.pages {
            view.pages
                .clone_from(self.pages.as_ref().unwrap_or(&base.pages));
        }
        if self.paint_order != previous.paint_order {
            view.paint_order.clone_from(self.paint_order.as_ref().unwrap_or(&base.paint_order));
        }
        if self.assets != previous.assets {
            view.assets
                .clone_from(self.assets.as_ref().unwrap_or(&base.assets));
        }
        if self.styles != previous.styles {
            view.styles
                .clone_from(self.styles.as_ref().unwrap_or(&base.styles));
        }
        if self.native_ui != previous.native_ui {
            view.native_ui.clone_from(self.native_ui.as_ref().unwrap_or(&base.native_ui));
        }
        if self.unsupported != previous.unsupported {
            view.unsupported
                .clone_from(self.unsupported.as_ref().unwrap_or(&base.unsupported));
        }
        if self.icon != previous.icon {
            (view.icon, view.icon_hidden) = self.icon.unwrap_or((base.icon, base.icon_hidden));
        }
        let count = self.widget_count.unwrap_or(base.widgets.len());
        view.widgets.truncate(count);
        while view.widgets.len() < count {
            let n = view.widgets.len();
            let widget = self
                .widgets
                .iter()
                .find(|(at, _)| *at == n)
                .map(|(_, w)| w)
                .or_else(|| base.widgets.get(n));
            let Some(widget) = widget else { break };
            view.widgets.push(widget.clone());
        }
        for (n, _) in &previous.widgets {
            if *n < count
                && !self.widgets.iter().any(|(at, _)| at == n)
                && let Some(w) = base.widgets.get(*n)
                && let Some(old) = view.widgets.get_mut(*n)
            {
                old.clone_from(w);
            }
        }
        for (n, w) in &self.widgets {
            if previous.widgets.iter().any(|(at, old)| at == n && old == w) {
                continue;
            }
            if let Some(old) = view.widgets.get_mut(*n) {
                old.clone_from(w);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authored_order_publication_preserves_all_three_states_and_reverts() {
        for base_order in [None, Some(vec![]), Some(vec![WidgetRef(1), WidgetRef(0)])] {
            let base = Interface {
                paint_order: base_order,
                pages: vec![Page { name: String::new(), size: Size { width: 100., height: 100. }, background: Default::default(), height_rows: None }],
                widgets: vec![
                    Widget::new("first", PageRef(0), Rect::new(0, 0, 10, 10), Kind::Label),
                    Widget::new("second", PageRef(0), Rect::new(10, 10, 10, 10), Kind::Label),
                ],
                ..Default::default()
            };
            let mut view = base.clone();
            let mut previous = InterfacePatch::default();
            for order in [Some(vec![WidgetRef(0), WidgetRef(1)]), Some(vec![]), None, base.paint_order.clone()] {
                let current = Interface { paint_order: order, ..base.clone() };
                let patch = InterfacePatch::between(&base, &current);
                assert!(patch.widgets.is_empty(), "order cannot renumber widget identities");
                patch.apply(&base, &previous, &mut view);
                assert_eq!(view, current, "order-only publication must reach the view");
                assert_eq!(InterfacePatch::between(&base, &view), patch);
                previous = patch;
            }
            assert_eq!(previous, InterfacePatch::default());
        }
    }

    #[test]
    fn fractional_geometry_and_reparenting_publish_without_renumbering() {
        let mut base = Interface::default();
        base.pages.push(Page { name: String::new(), size: Size { width: 300.5, height: 200.25 }, background: Default::default(), height_rows: None });
        base.widgets.push(Widget::new("first", PageRef(0), Rect { x: 5.5, y: 6.25, width: 100.75, height: 80.5 }, Kind::Panel));
        base.widgets.push(Widget::new("second", PageRef(0), Rect { x: 100.125, y: 20.75, width: 60.5, height: 40.25 }, Kind::Panel));
        let mut child = Widget::new("child", PageRef(0), Rect { x: 1.25, y: 2.5, width: 10.125, height: 9.75 }, Kind::Label);
        child.parent = Some(WidgetRef(0));
        base.widgets.push(child);
        base.paint_order = Some(vec![WidgetRef(0), WidgetRef(2), WidgetRef(1)]);
        let mut current = base.clone();
        current.pages[0].size.height = 210.875;
        current.widgets[2].parent = Some(WidgetRef(1));
        current.widgets[2].rect.x = 3.375;
        current.paint_order = Some(vec![WidgetRef(0), WidgetRef(1), WidgetRef(2)]);
        let patch = InterfacePatch::between(&base, &current);
        assert_eq!(patch.widgets.len(), 1);
        assert_eq!(patch.widgets[0].0, 2);
        let mut view = base.clone();
        patch.apply(&base, &Default::default(), &mut view);
        assert_eq!(view, current);
        assert_eq!(view.page_rect(WidgetRef(2)), Rect { x: 103.5, y: 23.25, width: 10.125, height: 9.75 });
        InterfacePatch::default().apply(&base, &patch, &mut view);
        assert_eq!(view, base);
    }

    #[test]
    fn performance_intent_is_sparse_and_reverts_to_the_authored_value() {
        let base = Interface::default();
        let current = Interface { performance: true, ..base.clone() };
        let patch = InterfacePatch::between(&base, &current);
        assert_eq!(patch, InterfacePatch { performance: Some(true), ..Default::default() });
        let mut view = base.clone();
        patch.apply(&base, &InterfacePatch::default(), &mut view);
        assert_eq!(view, current);
        InterfacePatch::default().apply(&base, &patch, &mut view);
        assert_eq!(view, base);
        assert_eq!(InterfacePatch::between(&base, &base), InterfacePatch::default());
    }
}
