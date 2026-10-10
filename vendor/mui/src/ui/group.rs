//! State queries for styling a named group from its authored descendants.
use super::*;

impl Ui {
    /// Whether the pointer is over this named group or one of its descendants.
    /// Use with `Paints::when` to style a row when a nested control is hovered.
    /// Reads the last resolved scene, like `Ui::get`; a floating descendant
    /// belongs to its authored group even when it is outside the group's bounds.
    pub fn group_hovered(&self, id: impl Into<Id>) -> bool {
        self.in_group(id.into().as_str(), self.interaction.hovered())
    }
    /// Whether this named group or a descendant owns the pointer gesture.
    pub fn group_held(&self, id: impl Into<Id>) -> bool {
        self.in_group(id.into().as_str(), self.interaction.held())
    }
    /// Whether this named group contains the currently focused element.
    pub fn group_focused(&self, id: impl Into<Id>) -> bool {
        self.in_group(id.into().as_str(), self.focus.as_deref())
    }
    /// Whether this named group contains keyboard or programmatic focus.
    /// Pointer focus does not light the group's focus ring.
    pub fn group_focus_visible(&self, id: impl Into<Id>) -> bool {
        self.focus_visible && self.group_focused(id)
    }
    fn in_group(&self, group: &str, target: Option<&str>) -> bool {
        let Some(scene) = &self.scene else {
            return false;
        };
        let mut at = target.and_then(|key| scene.surface(key));
        while let Some(surface) = at {
            if surface.disabled {
                return false;
            }
            if surface.key.as_str() == group {
                return true;
            }
            at = surface.parent.as_deref().and_then(|key| scene.surface(key));
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;

    #[test]
    fn groups_follow_authored_ancestry_and_focus_modality() {
        let tree = || {
            row![
                stack![block(20., 20.).a11y(A11y::Button).focusable().id("child")].id("group"),
                block(20., 20.).a11y(A11y::Button).id("groupish")
            ]
        };
        let mut ui = Ui::default();
        assert!(!ui.group_hovered("group"));
        ui.set_motion_policy(MotionPolicy::Reduced);
        ui.frame(tree(), None, Input::default(), 0.).unwrap();
        let input = PointerInput {
            pos: Some(Point::new(10., 10.)),
            buttons: Buttons::PRIMARY,
            ..PointerInput::default()
        };
        ui.frame(tree(), None, input, 0.).unwrap();
        assert!(ui.group_hovered("group"));
        assert!(ui.group_held("group"));
        assert!(ui.group_focused("group"));
        assert!(!ui.group_focus_visible("group"), "pointer focus");
        assert!(!ui.group_hovered("groupish"), "prefix is not ancestry");
        ui.focus("child");
        assert!(ui.group_focus_visible("group"), "programmatic focus");
        ui.frame(tree().disabled(), None, Input::default(), 0.)
            .unwrap();
        assert!(!ui.group_focused("group"));
        assert!(!ui.group_held("group"));

        let floating = || {
            stack![
                stack![
                    block(20., 20.)
                        .a11y(A11y::Button)
                        .id("float")
                        .float()
                        .pin(Pin::to("group").area(Area::End).gap(5.))
                ]
                .size(20., 20.)
                .id("group")
            ]
            .size(100., 40.)
        };
        ui.frame(floating(), None, Input::default(), 0.).unwrap();
        ui.frame(
            floating(),
            None,
            PointerInput {
                pos: Some(Point::new(75., 20.)),
                ..PointerInput::default()
            },
            0.,
        )
        .unwrap();
        assert!(
            ui.group_hovered("group"),
            "floating children retain authored ancestry"
        );
    }
}
