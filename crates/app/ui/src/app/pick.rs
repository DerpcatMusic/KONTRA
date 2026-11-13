//! The file a part's script asks for (a library's own Save and Load), picked
//! among those saved for its instrument: KONTRA shows this sheet, not the
//! system's file dialog.

use mui::prelude::*;

use super::{Action, App, FilePick};
use crate::kit::{self, button, list_row};
use crate::theme::{CONTROL, SPACE, secondary};

/// The rows the list shows at once; more scroll.
const ROWS: f64 = 8.;

/// The sheet for `pick`: the names saved so far, and, to save, a name to
/// type (a saved one clicked fills it). Open takes a name clicked.
pub(super) fn sheet(ui: &mut Ui, app: &mut App, pick: FilePick, out: &mut Vec<Action>) -> El {
    // A new pick starts empty, its field focused.
    if app.view.picking.as_ref() != Some(&pick) {
        app.view.pick_name.clear();
        app.view.picking = Some(pick.clone());
        ui.focus("pick-name");
    }
    let mut answer = None;
    let rows: Vec<El> = (pick.names.iter().enumerate())
        .map(|(n, name)| {
            let shown = app.view.pick_name == *name;
            let r = list_row(ui, format!("pick-{n}"), (name, ""), None, shown);
            if r.changed {
                app.view.pick_name.clone_from(name);
                if !pick.save {
                    answer = Some(Some(name.clone()));
                }
            }
            r.el
        })
        .collect();
    let list = match rows.is_empty() {
        true => caption("Nothing saved for this instrument yet.").fill(secondary()),
        false => (col(rows).gap(1).w(Len::Pct(100.)))
            .scroll()
            .h(ROWS * (CONTROL + 1.))
            .w(Len::Pct(100.))
            .min_h(0),
    };
    let mut items = vec![list];
    let verb = if pick.save { "Save" } else { "Open" };
    let mut submitted = false;
    if pick.save {
        let field = kit::field(ui, "pick-name", "Name", &mut app.view.pick_name);
        submitted = field.changed.submitted;
        items.push(field.el);
    }
    let go = button(ui, "pick-go", verb, false);
    let name = app.view.pick_name.trim().to_owned();
    if (go.changed || submitted) && !name.is_empty() {
        answer = Some(Some(name));
    }
    items.push(go.el);
    let title = if pick.save { "Save file" } else { "Open file" };
    let content = col(items).gap(SPACE).w(Len::Pct(100.));
    let r = kit::sheet(ui, "pick", title, content);
    if r.changed {
        answer = Some(None);
    }
    if let Some(answer) = answer {
        out.push(Action::Picked(answer));
        app.pick = None;
        app.view.picking = None;
    }
    r.el
}
