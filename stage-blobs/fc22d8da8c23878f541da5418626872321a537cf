//! Platform accessibility requests, independent of pointer hit testing.

/// A host asks for an operation on an existing, enabled semantic control.
/// Queue with `Ui::request_action`, then build the normal widget tree and frame.
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticAction {
    Focus {
        id: String,
    },
    Activate {
        id: String,
    },
    /// Value in the control's semantic units, not an implicitly normalized value.
    SetValue {
        id: String,
        value: f64,
    },
    /// One step up a slider, the step its arrow keys take: see
    /// [`crate::widgets::step`]. Lands as the equivalent `SetValue`.
    Increment {
        id: String,
    },
    /// One step down; see [`SemanticAction::Increment`].
    Decrement {
        id: String,
    },
    /// Select in a text field: the anchor and the caret, in characters of
    /// its value, equal for a bare caret. AccessKit's `SetTextSelection`,
    /// whose character indices are these.
    SetSelection {
        id: String,
        anchor: usize,
        caret: usize,
    },
}
impl SemanticAction {
    pub fn focus(id: impl Into<String>) -> Self {
        Self::Focus { id: id.into() }
    }
    pub fn activate(id: impl Into<String>) -> Self {
        Self::Activate { id: id.into() }
    }
    pub fn set_value(id: impl Into<String>, value: f64) -> Self {
        Self::SetValue {
            id: id.into(),
            value,
        }
    }
    pub fn increment(id: impl Into<String>) -> Self {
        Self::Increment { id: id.into() }
    }
    pub fn decrement(id: impl Into<String>) -> Self {
        Self::Decrement { id: id.into() }
    }
    pub fn set_selection(id: impl Into<String>, anchor: usize, caret: usize) -> Self {
        Self::SetSelection {
            id: id.into(),
            anchor,
            caret,
        }
    }
    pub(crate) fn id(&self) -> &str {
        match self {
            Self::Focus { id }
            | Self::Activate { id }
            | Self::SetValue { id, .. }
            | Self::Increment { id }
            | Self::Decrement { id }
            | Self::SetSelection { id, .. } => id,
        }
    }
}
