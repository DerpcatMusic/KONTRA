//! What a part's script just named: texts its interface began to show (a
//! menu's selected item, a label's or a text edit's text) and the stems of
//! the array files it loaded. The editor's header follows the script by
//! these names: one equal to a preset of the instrument is the preset that
//! plays, whatever route the script took to it.

use kontra_ui_model as model;

use super::Rack;

/// A part's shown texts, and what it named since it was last asked.
#[derive(Debug, Default)]
pub struct Named {
    /// Each widget's text as last shown, by widget.
    shown: Vec<String>,
    /// Texts that began to show, and stems of files loaded, since the last
    /// [`Rack::take_named`].
    new: Vec<String>,
}

impl Named {
    /// Notes the texts `tree` shows that it did not before. The first look
    /// at an interface only learns what it shows.
    pub(crate) fn look(&mut self, tree: &model::Interface) {
        let first = self.shown.is_empty();
        self.shown.resize(tree.widgets.len(), String::new());
        for (w, was) in tree.widgets.iter().zip(&mut self.shown) {
            let now = shown(w);
            if was.as_str() != now {
                if !first && !now.trim().is_empty() {
                    self.new.push(now.to_owned());
                }
                now.clone_into(was);
            }
        }
    }

    /// Notes a file the script loaded, by its stem.
    pub(crate) fn loaded(&mut self, path: &str) {
        let stem = std::path::Path::new(path).file_stem();
        self.new
            .extend(stem.map(|s| s.to_string_lossy().into_owned()));
    }
}

/// The text a widget shows: a menu's selected item, else its text.
fn shown(w: &model::Widget) -> &str {
    match (&w.kind, &w.value) {
        (model::Kind::Menu { items }, Some(model::Value::Integer(at))) => {
            (items.iter().find(|i| i.value == *at)).map_or("", |i| i.text.as_str())
        }
        _ => &w.text,
    }
}

impl Rack {
    /// The names `part`'s script showed or loaded since the last call.
    pub fn take_named(&mut self, part: usize) -> Vec<String> {
        let p = self.parts.get_mut(part).and_then(Option::as_mut);
        p.map(|p| std::mem::take(&mut p.named.new))
            .unwrap_or_default()
    }
}
