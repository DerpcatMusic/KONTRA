//! What a program asks for that this crate does not play: active insert
//! effects, aux sends and modulation connections nothing carried, reported as
//! diagnostics after the translation.

use kontra_ir as ir;

use super::{
    Library, PAN, PITCH, Translator, filters, flag, held_macro, modulation, num, routing,
    source_node,
};

impl<L: Library> Translator<'_, '_, L> {
    /// A step envelope's `Smooth` is not applied to the routes carried from
    /// it (`modulation::steps`): one source, reported once.
    pub(super) fn step_smoothing(&mut self) {
        let program = self.program;
        for node in program
            .descendants()
            .filter(|n| n.has_tag_name("StepEnvelope"))
        {
            let carried = program.descendants().any(|c| {
                self.carried.contains(&c.id()) && source_node(program, c).is_some_and(|n| n == node)
            });
            if carried && !flag(node, "Bypass") && num(node, "Smooth", 0.0).is_ok_and(|s| s > 1e-3)
            {
                let name = node.attribute("Name").unwrap_or_default();
                self.notes
                    .add(ir::Impact::Audible, "StepEnvelope Smooth", name);
            }
        }
    }

    /// Active insert effects and aux sends this crate does not carry.
    pub(super) fn effects(&mut self) {
        for node in self.program.descendants().filter(|n| n.is_element()) {
            if flag(node, "Bypass") {
                continue;
            }
            let tag = node.tag_name().name();
            let parent = node.parent();
            // A keygroup's GainMatrix places its bundle members (`placement`).
            let placing = tag == "GainMatrix"
                && parent
                    .and_then(|p| p.parent())
                    .is_some_and(|p| p.has_tag_name("Keygroup"));
            if parent.is_some_and(|p| p.has_tag_name("Inserts"))
                && !routing::carried(node)
                && !filters::carried(node)
                && !placing
            {
                self.notes.add(
                    routing::impact(self.program, node),
                    &format!("insert {tag}"),
                    node.attribute("Name").unwrap_or_default(),
                );
            } else if tag == "BusRouter"
                && !parent
                    .and_then(|p| p.parent())
                    .is_some_and(|p| p.has_tag_name("Layer"))
                && num(node, "Gain", 1.0).is_ok_and(|g| g > 0.0)
            {
                self.notes.add(
                    ir::Impact::Audible,
                    "aux send",
                    node.attribute("Destination").unwrap_or_default(),
                );
            }
        }
    }

    /// Live modulation (LFOs, scripts, controllers) of gain, pan, pitch and the
    /// matrix. The amplitude envelope is carried by [`Self::envelope`] and is skipped.
    pub(super) fn connections(&mut self) {
        for connection in self
            .program
            .descendants()
            .filter(|n| n.has_tag_name("SignalConnection"))
        {
            // What moves a bypassed module (or through a bypassed connection) is moot.
            if connection.ancestors().any(|a| flag(a, "Bypass")) {
                continue;
            }
            // At ratio 0 a connection moves nothing: what modulates its
            // ratio scales it (matrix.rs), so 0 stays 0.
            if num(connection, "Ratio", 1.0).is_ok_and(|r| r == 0.0) {
                continue;
            }
            if modulation::script_route(self.program, connection).is_some()
                || self.routed.contains_key(&connection.id())
                || modulation::gain_route(self.program, connection).is_some()
                || [PITCH, PAN]
                    .into_iter()
                    .any(|to| modulation::route(self.program, connection, to).is_some())
            {
                continue;
            }
            let dest = connection.attribute("Destination").unwrap_or_default();
            let source = connection.attribute("Source").unwrap_or_default();
            let envelope =
                source_node(self.program, connection).is_some_and(|n| n.has_tag_name("DAHDSR"));
            if dest == "Gain" && envelope && num(connection, "Ratio", 0.0).is_ok_and(|r| r == 1.0) {
                continue;
            }
            if self.carried.contains(&connection.id()) {
                continue;
            }
            // An edit-mode connection (ConnectionMode 1) from a macro sets the
            // knob when the macro moves; at load the saved value stands (the
            // comb's Freq 39.905 is exactly the macro's 0.1 through its law).
            // ponytail: the macro's own moves are not wired to the knob yet.
            let owner = connection.parent().and_then(|c| c.parent());
            if flag(connection, "ConnectionMode")
                && held_macro(connection)
                && owner.is_some_and(|o| routing::carried(o) || filters::carried(o))
            {
                continue;
            }
            let feature = if dest.starts_with("Gain_") {
                "matrix cell modulation"
            } else {
                "modulation"
            };
            let impact = routing::impact(self.program, connection);
            self.notes.add(impact, &format!("{feature} {dest}"), source);
        }
    }
}
