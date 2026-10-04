//! Cocoa NSTextInputClient state. Native callbacks release every RefCell borrow before dispatch.
use crate::{ime::utf16_to_byte, ImeConfiguration};
use keyboard_types::KeyboardEvent;
use objc2::{msg_send, runtime::AnyObject, ClassType};
use objc2_foundation::{NSAttributedString, NSRange, NSString};
use std::cell::{Cell, RefCell};

#[derive(Default)]
pub(crate) struct NativeIme {
    pub configuration: RefCell<Option<ImeConfiguration>>,
    pub marked: RefCell<String>,
    pub shadow: RefCell<Option<crate::ime::TextShadow>>,
    pub discarding: Cell<bool>,
    pub focused: Cell<bool>,
    pub pending_key: RefCell<Option<KeyboardEvent>>,
}
impl NativeIme {
    pub fn enabled(&self) -> bool {
        self.focused.get() && !self.discarding.get() && self.configuration.borrow().is_some()
    }
    pub fn configure(&self, config: &Option<ImeConfiguration>) {
        let previous = self.configuration.borrow().clone();
        let mut shadow = self.shadow.borrow_mut();
        match (previous.as_ref(), config.as_ref(), shadow.as_mut()) {
            (Some(previous), Some(current), Some(shadow)) => shadow.reconcile(previous, current),
            (_, Some(current), _) => *shadow = Some(crate::ime::TextShadow::new(current)),
            (_, None, _) => *shadow = None,
        }
        drop(shadow);
        *self.configuration.borrow_mut() = config.clone();
    }
    pub fn selection(&self) -> NSRange {
        self.shadow
            .borrow()
            .as_ref()
            .and_then(|shadow| shadow.selection_utf16())
            .map_or(NSRange::new(crate::ime::NS_NOT_FOUND, 0), |(at, len)| NSRange::new(at, len))
    }
    pub fn marked_range(&self) -> NSRange {
        let shadow = self.shadow.borrow();
        let Some(shadow) = shadow.as_ref() else {
            return NSRange::new(crate::ime::NS_NOT_FOUND, 0);
        };
        shadow.marked.as_ref().map_or(NSRange::new(crate::ime::NS_NOT_FOUND, 0), |range| {
            NSRange::new(
                shadow.text[..range.start].encode_utf16().count(),
                shadow.text[range.clone()].encode_utf16().count(),
            )
        })
    }
    pub fn replacement(&self, range: NSRange) -> Option<crate::Ime> {
        self.shadow
            .borrow()
            .as_ref()?
            .utf16_range(range.location, range.length)
            .map(crate::Ime::Selection)
    }
    pub fn commit(&self, text: &str, replacement: NSRange) {
        if let Some(shadow) = self.shadow.borrow_mut().as_mut() {
            let replacement = shadow.utf16_range(replacement.location, replacement.length);
            shadow.replace(text, replacement, None, false);
        }
        self.marked.borrow_mut().clear();
    }
    pub fn mark(&self, text: String, selected: NSRange, replacement: NSRange) -> crate::Ime {
        let cursor = if selected.location >= crate::ime::NS_NOT_FOUND {
            None
        } else {
            Some(
                utf16_to_byte(&text, selected.location)
                    ..utf16_to_byte(&text, selected.location.saturating_add(selected.length)),
            )
        };
        if let Some(shadow) = self.shadow.borrow_mut().as_mut() {
            let replacement = shadow.utf16_range(replacement.location, replacement.length);
            shadow.replace(&text, replacement, cursor.clone(), true);
        }
        *self.marked.borrow_mut() = text.clone();
        crate::Ime::Preedit { text, cursor: cursor.map(|r| (r.start, r.end)) }
    }
}

/// AppKit supplies NSString or NSAttributedString; `string` must precede UTF8 extraction.
pub(super) fn text(object: &AnyObject) -> String {
    unsafe {
        let attributed: bool = msg_send![object, isKindOfClass: NSAttributedString::class()];
        let string: &NSString = if attributed {
            msg_send![object, string]
        } else {
            &*(object as *const AnyObject).cast::<NSString>()
        };
        string.to_string()
    }
}
