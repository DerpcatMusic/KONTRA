//! Native text input shared contract. Platform implementations own their native resources.
use crate::dpi::{PhysicalPosition, PhysicalSize};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub enum Ime {
    Enabled,
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    Commit(String),
    Disabled,
    /// Replace this UTF-8 byte range before the next preedit or commit.
    Selection(Range<usize>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImeConfiguration {
    /// Stable focused input identity; changing it cancels the previous composition.
    pub id: String,
    pub position: PhysicalPosition<f64>,
    pub size: PhysicalSize<f64>,
    pub text: String,
    pub selection: Range<usize>,
    /// Marked range in `text`, in UTF-8 bytes.
    pub marked: Option<Range<usize>>,
}

impl ImeConfiguration {
    /// Reject invalid geometry and selections rather than passing them to native APIs.
    pub(crate) fn valid(&self) -> bool {
        self.position.x.is_finite()
            && self.position.y.is_finite()
            && self.size.width.is_finite()
            && self.size.height.is_finite()
            && self.size.width >= 0.0
            && self.size.height >= 0.0
            && self.selection.start <= self.selection.end
            && self.text.is_char_boundary(self.selection.start)
            && self.text.is_char_boundary(self.selection.end)
            && self.marked.as_ref().is_none_or(|r| {
                r.start <= r.end
                    && self.text.is_char_boundary(r.start)
                    && self.text.is_char_boundary(r.end)
            })
    }
}

/// A model selection/value change can cancel native preedit before its first
/// frame is acknowledged. Geometry-only updates must leave it composing.
pub(crate) fn composition_cancelled(
    previous: &Option<ImeConfiguration>, current: &Option<ImeConfiguration>, composing: bool,
) -> bool {
    let (Some(previous), Some(current)) = (previous, current) else { return false };
    composing
        && previous.id == current.id
        && current.marked.is_none()
        && (previous.marked.is_some()
            || previous.text != current.text
            || previous.selection != current.selection)
}

/// IMM32 uses signed device coordinates. Float casts and rectangle sums must
/// saturate independently for valid but extreme candidate geometry.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn candidate_rect(config: &ImeConfiguration) -> (i32, i32, i32, i32) {
    let x = config.position.x as i32;
    let y = config.position.y as i32;
    (
        x,
        y,
        x.saturating_add(config.size.width as i32).max(x.saturating_add(1)),
        y.saturating_add(config.size.height as i32),
    )
}

#[cfg(any(target_os = "linux", test))]
pub(crate) fn char_to_byte(text: &str, index: usize) -> usize {
    text.char_indices().nth(index).map_or(text.len(), |(i, _)| i)
}

#[cfg(any(target_os = "windows", target_os = "macos", test))]
pub(crate) fn utf16_to_byte(text: &str, index: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units + ch.len_utf16() > index {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_preedit_cancels_on_authoritative_selection_even_before_first_frame() {
        let before = Some(ImeConfiguration {
            id: "field".into(),
            position: PhysicalPosition::new(0., 0.),
            size: PhysicalSize::new(1., 20.),
            text: "abc".into(),
            selection: 1..1,
            marked: None,
        });
        let mut after = before.clone();
        after.as_mut().unwrap().position.x = 20.;
        assert!(!composition_cancelled(&before, &after, true));
        after.as_mut().unwrap().selection = 0..0;
        assert!(composition_cancelled(&before, &after, true));
        assert!(!composition_cancelled(&before, &after, false));
        after.as_mut().unwrap().marked = Some(0..1);
        assert!(!composition_cancelled(&before, &after, true));
    }
    #[test]
    fn huge_finite_candidate_geometry_saturates_without_overflow() {
        let config = ImeConfiguration {
            id: "field".into(),
            position: PhysicalPosition::new(f64::MAX, f64::MAX),
            size: PhysicalSize::new(f64::MAX, f64::MAX),
            text: String::new(),
            selection: 0..0,
            marked: None,
        };
        assert!(config.valid());
        assert_eq!(candidate_rect(&config), (i32::MAX, i32::MAX, i32::MAX, i32::MAX));
        let mut negative = config;
        negative.position = PhysicalPosition::new(-f64::MAX, -f64::MAX);
        negative.size = PhysicalSize::new(0., 0.);
        assert_eq!(candidate_rect(&negative), (i32::MIN, i32::MIN, i32::MIN + 1, i32::MIN));
    }
    #[test]
    fn native_offsets_respect_surrogates_and_utf8() {
        let s = "a😀é";
        assert_eq!((char_to_byte(s, 2), utf16_to_byte(s, 3)), (5, 5));
        assert_eq!(utf16_to_byte(s, 2), 1);
        let mut config = ImeConfiguration {
            id: "field".into(),
            position: PhysicalPosition::new(3., 4.),
            size: PhysicalSize::new(1., 15.),
            text: s.into(),
            selection: 1..5,
            marked: None,
        };
        assert!(config.valid());
        assert_eq!(Some(config.clone()), Some(config.clone()), "unchanged config compares equal");
        config.selection = 2..5;
        assert!(!config.valid());
        config.selection = 1..5;
        config.position.x = f64::NAN;
        assert!(!config.valid());
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) const NS_NOT_FOUND: usize = isize::MAX as usize;

/// Cocoa queries may run several times between UI ticks. Keep native edits visible
/// synchronously, and ignore the unchanged old frame until the model catches up.
#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug)]
pub(crate) struct TextShadow {
    pub text: String,
    pub selection: Range<usize>,
    pub marked: Option<Range<usize>>,
    pub selection_visible: bool,
    pub pending: bool,
}
#[cfg(any(target_os = "macos", test))]
impl TextShadow {
    pub fn new(config: &ImeConfiguration) -> Self {
        Self {
            text: config.text.clone(),
            selection: config.selection.clone(),
            marked: config.marked.clone(),
            selection_visible: true,
            pending: false,
        }
    }
    pub fn reconcile(&mut self, previous: &ImeConfiguration, current: &ImeConfiguration) {
        let same_input = previous.id == current.id;
        let old_frame = current.text == previous.text
            && current.selection == previous.selection
            && current.marked == previous.marked;
        let acknowledged = current.text == self.text
            && current.selection == self.selection
            && current.marked == self.marked;
        if same_input && acknowledged {
            // A model acknowledgement must preserve native NSNotFound cursor
            // visibility while marked text remains active.
            self.pending = false;
        } else if !(same_input && self.pending && old_frame) {
            *self = Self::new(current);
        }
    }
    pub fn utf16_range(&self, at: usize, len: usize) -> Option<Range<usize>> {
        if at >= NS_NOT_FOUND {
            return None;
        }
        Some(utf16_to_byte(&self.text, at)..utf16_to_byte(&self.text, at.saturating_add(len)))
    }
    pub fn selection_utf16(&self) -> Option<(usize, usize)> {
        if !self.selection_visible {
            return None;
        }
        Some((
            self.text[..self.selection.start].encode_utf16().count(),
            self.text[self.selection.clone()].encode_utf16().count(),
        ))
    }
    pub fn replace(
        &mut self, text: &str, range: Option<Range<usize>>, cursor: Option<Range<usize>>,
        preedit: bool,
    ) {
        let range = range.or_else(|| self.marked.clone()).unwrap_or_else(|| self.selection.clone());
        let start = range.start;
        self.text.replace_range(range, text);
        let end = start + text.len();
        self.marked = preedit.then_some(start..end).filter(|r| !r.is_empty());
        self.selection_visible = !preedit || cursor.is_some();
        self.selection = cursor.map_or(end..end, |r| start + r.start..start + r.end);
        self.pending = true;
    }
}

#[cfg(test)]
mod shadow_tests {
    use super::*;
    fn configuration() -> ImeConfiguration {
        ImeConfiguration {
            id: "field".into(),
            position: PhysicalPosition::new(0., 0.),
            size: PhysicalSize::new(1., 20.),
            text: "a😀z".into(),
            selection: 1..5,
            marked: None,
        }
    }
    #[test]
    fn rapid_native_edits_have_live_text_and_safe_utf16_ranges() {
        let config = configuration();
        let mut shadow = TextShadow::new(&config);
        shadow.replace("日本", None, Some(3..3), true);
        assert_eq!(shadow.text, "a日本z");
        assert_eq!(shadow.selection_utf16(), Some((2, 0)));
        assert_eq!(shadow.utf16_range(1, 2), Some(1..7));
        shadow.reconcile(&config, &config);
        assert_eq!(shadow.text, "a日本z", "old frame cannot overwrite pending composition");
        // The next native callback changes the marked text before any UI tick.
        shadow.replace("語😀", None, None, true);
        assert_eq!(shadow.text, "a語😀z");
        assert_eq!(shadow.selection_utf16(), None, "NSNotFound hides selection safely");
        assert_eq!(shadow.utf16_range(NS_NOT_FOUND, usize::MAX), None);
        assert_eq!(shadow.utf16_range(usize::MAX, usize::MAX), None);
        assert_eq!(shadow.utf16_range(2, 2), Some(4..8));
        shadow.replace("語😀", None, None, false);
        assert_eq!(shadow.text, "a語😀z", "commit replaces marked text exactly once");
        assert_eq!(shadow.marked, None);
        assert_eq!(shadow.selection_utf16(), Some((4, 0)));
        let mut acknowledged = config.clone();
        acknowledged.text = shadow.text.clone();
        acknowledged.selection = shadow.selection.clone();
        shadow.reconcile(&config, &acknowledged);
        assert!(!shadow.pending);
        let next = shadow.utf16_range(4, 1);
        shadow.replace("é", next, None, false);
        assert_eq!(shadow.text, "a語😀é", "second replacement reads the live buffer");
    }
    #[test]
    fn acknowledged_preedit_keeps_native_hidden_selection() {
        let config = configuration();
        let mut shadow = TextShadow::new(&config);
        shadow.replace("日本", None, None, true);
        let mut acknowledged = config.clone();
        acknowledged.text = shadow.text.clone();
        acknowledged.selection = shadow.selection.clone();
        acknowledged.marked = shadow.marked.clone();
        shadow.reconcile(&config, &acknowledged);
        assert!(!shadow.pending);
        assert_eq!(shadow.selection_utf16(), None);
        let mut moved = acknowledged.clone();
        moved.position.x = 20.;
        shadow.reconcile(&acknowledged, &moved);
        assert_eq!(shadow.selection_utf16(), None, "geometry must not reveal hidden native caret");
        shadow.replace("日本", None, None, false);
        assert!(shadow.selection_utf16().is_some());
    }
    #[test]
    fn new_focus_and_model_edits_replace_pending_native_shadow() {
        let config = configuration();
        let mut shadow = TextShadow::new(&config);
        shadow.replace("漢", None, Some(3..3), true);
        let mut other = config.clone();
        other.id = "other".into();
        shadow.reconcile(&config, &other);
        assert_eq!(shadow.text, "a😀z");
        assert!(!shadow.pending);
        shadow.replace("漢", None, Some(3..3), true);
        other.text = "replacement".into();
        other.selection = 0..0;
        shadow.reconcile(&config, &other);
        assert_eq!(shadow.text, "replacement");
    }
}
