//! Sparse cumulative changes relative to a generation's immutable authored interface.
use super::*;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InterfacePatch {
    pub source: Option<Source>,
    pub performance: Option<bool>,
    pub pages: Option<Vec<Page>>,
    pub widgets: Vec<(usize, Widget)>,
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
            native_ui: (base.native_ui != current.native_ui).then(|| current.native_ui.clone()),
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
        if self.assets != previous.assets {
            view.assets
                .clone_from(self.assets.as_ref().unwrap_or(&base.assets));
        }
        if self.styles != previous.styles {
            view.styles
                .clone_from(self.styles.as_ref().unwrap_or(&base.styles));
        }
        if self.native_ui != previous.native_ui {
            view.native_ui
                .clone_from(self.native_ui.as_ref().unwrap_or(&base.native_ui));
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
