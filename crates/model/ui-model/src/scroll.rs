//! Regions a source asks to clip to their bounds and scroll inside (a UVI
//! `Viewport`): [`Widget::viewport`] marks one, its children are its content.

use crate::{Interface, Size, Widget, WidgetRef};

impl Interface {
    /// The nearest scrolling region `widget` sits inside, if any.
    #[must_use]
    pub fn scroller(&self, widget: WidgetRef) -> Option<WidgetRef> {
        let mut at = self.widgets.get(widget.0)?.parent;
        for _ in 0..self.widgets.len() {
            let p = at?;
            let w = self.widgets.get(p.0)?;
            if w.viewport.is_some() {
                return Some(p);
            }
            at = w.parent;
        }
        None
    }

    /// How far the content of the region `viewport` reaches from its
    /// origin: its own size at least, else its children's farthest edges.
    #[must_use]
    pub fn content(&self, viewport: WidgetRef) -> Size {
        let own = self.widgets.get(viewport.0).map(|w| w.rect);
        let children = self.widgets.iter().filter(|w| w.parent == Some(viewport));
        children.fold(
            own.map_or(Size::default(), |r| Size {
                width: r.width,
                height: r.height,
            }),
            |s, w: &Widget| Size {
                width: s.width.max(w.rect.x + w.rect.width),
                height: s.height.max(w.rect.y + w.rect.height),
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::{Interface, Kind, PageRef, Rect, Widget, WidgetRef};

    #[test]
    fn a_region_holds_its_rows_and_reaches_as_far_as_they_do() {
        let at = |y, h| Rect {
            x: 0.,
            y,
            width: 100.,
            height: h,
        };
        let mut port = Widget::new("port", PageRef(0), at(40., 100.), Kind::Panel);
        port.viewport = Some([0, 0]);
        let mut list = Widget::new("list", PageRef(0), at(0., 400.), Kind::Panel);
        list.parent = Some(WidgetRef(0));
        let mut row = Widget::new("row", PageRef(0), at(380., 18.), Kind::Label);
        row.parent = Some(WidgetRef(1));
        let tree = Interface {
            widgets: vec![port, list, row],
            ..Interface::default()
        };
        assert_eq!(tree.scroller(WidgetRef(2)), Some(WidgetRef(0)));
        assert_eq!(tree.scroller(WidgetRef(0)), None);
        assert_eq!(tree.content(WidgetRef(0)).height, 400.);
    }
}
