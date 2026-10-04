//! XIM composition over the window's existing XCB connection; no global locale mutation.
//! Uses the MIT licensed xim-rs transport also used by GPUI. See README-MUI.md.
use crate::ime::char_to_byte;
use crate::wrappers::xlib::XlibXcbConnection;
use crate::{Event, Ime, ImeConfiguration};
use std::{collections::VecDeque, sync::Arc};
use x11rb::{
    protocol::{xproto::KeyPressEvent, Event as XEvent},
    xcb_ffi::XCBConnection,
};
use xim::x11rb::{HasConnection, X11rbClient};
use xim::{AttributeName, Client, ClientError, ClientHandler, InputStyle};

struct Connection(Arc<XlibXcbConnection>);
impl HasConnection for Connection {
    type Connection = XCBConnection;
    fn conn(&self) -> &XCBConnection {
        self.0.xcb_connection()
    }
}

pub(super) struct NativeIme {
    client: X11rbClient<Connection>,
    handler: Handler,
}
#[derive(Default)]
struct Handler {
    window: u32,
    im: u16,
    ic: u16,
    enabled: bool,
    focused: bool,
    preedit: String,
    caret: usize,
    generation: u64,
    creating: Option<u64>,
    retired: Vec<u16>,
    default_forward_mask: u32,
    default_synchronous_mask: u32,
    mask_ic: u16,
    individual_mask: bool,
    forward_mask: u32,
    synchronous_mask: u32,
    deferred_keys: VecDeque<KeyPressEvent>,
    synchronizing: bool,
    configuration: Option<ImeConfiguration>,
    pending: VecDeque<NativeEvent>,
}
pub(super) enum NativeEvent {
    Input(Event),
    Key(KeyPressEvent),
}

impl NativeIme {
    pub fn new(connection: Arc<XlibXcbConnection>, window: u32) -> Option<Self> {
        let screen = connection.default_screen_index().into();
        match X11rbClient::init(Connection(connection), screen, None) {
            Ok(client) => Some(Self { client, handler: Handler { window, ..Handler::default() } }),
            Err(e) => {
                crate::warn!("XIM unavailable: {} (set XMODIFIERS before process startup)", e);
                None
            }
        }
    }
    pub fn configure(&mut self, config: Option<ImeConfiguration>, focused: bool) {
        let config = config.filter(ImeConfiguration::valid);
        if self.handler.configuration == config && self.handler.focused == focused {
            return;
        }
        let was_enabled = self.handler.enabled;
        let cancelled = crate::ime::composition_cancelled(
            &self.handler.configuration,
            &config,
            !self.handler.preedit.is_empty(),
        );
        let switched =
            self.handler.configuration.as_ref().map(|c| &c.id) != config.as_ref().map(|c| &c.id);
        self.handler.configuration = config;
        self.handler.focused = focused;
        self.handler.enabled = self.handler.configuration.is_some() && focused;
        if switched || cancelled || was_enabled != self.handler.enabled {
            if let Some(old_ic) = self.handler.begin_generation() {
                // Retain the retired ID until DestroyIcReply. A new context is
                // requested only afterwards, so ID reuse cannot route old text
                // into the new field. ResetIc on a reused context is insufficient.
                let _ = self.client.unset_focus(self.handler.im, old_ic);
                if let Err(e) = self.client.destroy_ic(self.handler.im, old_ic) {
                    crate::warn!("XIM context retirement failed: {}", e);
                }
            }
            if was_enabled && self.handler.enabled {
                self.handler.emit(Ime::Disabled);
            }
        }
        if let Err(e) = self.handler.create_if_ready(&mut self.client) {
            crate::warn!("XIM context creation failed: {}", e);
        }
        if self.handler.ic != 0 && self.handler.enabled {
            if let Err(e) = self.handler.set_spot(&mut self.client) {
                crate::warn!("XIM configuration failed: {}", e);
            }
        }
        if was_enabled != self.handler.enabled || ((switched || cancelled) && self.handler.enabled)
        {
            if !self.handler.enabled {
                self.handler.preedit.clear();
            }
            self.handler.emit(if self.handler.enabled { Ime::Enabled } else { Ime::Disabled });
        }
    }
    pub fn filter(&mut self, event: &XEvent) -> bool {
        match self.client.filter_event(event, &mut self.handler) {
            Ok(filtered) => filtered,
            Err(e) => {
                crate::warn!("XIM protocol error: {}", e);
                false
            }
        }
    }
    pub fn forward(&mut self, event: &KeyPressEvent) -> bool {
        let client = &mut self.client;
        match self
            .handler
            .forward(event, |im, ic, flags, event| client.forward_event(im, ic, flags, event))
        {
            Ok(forwarded) => forwarded,
            Err(e) => {
                crate::warn!("XIM key forwarding failed: {}", e);
                false
            }
        }
    }

    pub fn drain(&mut self) -> VecDeque<NativeEvent> {
        std::mem::take(&mut self.handler.pending)
    }
}
impl Drop for NativeIme {
    fn drop(&mut self) {
        if self.handler.ic != 0 {
            let _ = self.client.destroy_ic(self.handler.im, self.handler.ic);
        }
        if self.handler.im != 0 {
            let _ = self.client.close(self.handler.im);
        }
        let _ = self.client.disconnect();
    }
}
impl Handler {
    fn current(&self, im: u16, ic: u16) -> bool {
        self.im == im && self.ic != 0 && self.ic == ic
    }
    fn begin_generation(&mut self) -> Option<u16> {
        self.generation = self.generation.wrapping_add(1);
        self.deferred_keys.clear();
        self.synchronizing = false;
        self.preedit.clear();
        self.caret = 0;
        self.individual_mask = false;
        self.mask_ic = 0;
        self.forward_mask = self.default_forward_mask;
        self.synchronous_mask = self.default_synchronous_mask;
        let old = std::mem::replace(&mut self.ic, 0);
        if old == 0 {
            None
        } else {
            self.retired.push(old);
            Some(old)
        }
    }
    fn accepts(&self, im: u16, ic: u16) -> bool {
        self.enabled && self.current(im, ic) && !self.retired.contains(&ic)
    }
    fn activate_created(&mut self, im: u16, ic: u16) -> bool {
        if im != self.im {
            return false;
        }
        let generation = self.creating.take();
        if generation != Some(self.generation) || !self.enabled || self.retired.contains(&ic) {
            if im == self.im && !self.retired.contains(&ic) {
                self.retired.push(ic);
            }
            return false;
        }
        self.ic = ic;
        if self.mask_ic != ic {
            self.mask_ic = ic;
            self.individual_mask = false;
            self.forward_mask = self.default_forward_mask;
            self.synchronous_mask = self.default_synchronous_mask;
        }
        true
    }
    fn create_if_ready<C: Client>(&mut self, c: &mut C) -> Result<(), ClientError> {
        if !self.enabled
            || self.im == 0
            || self.ic != 0
            || self.creating.is_some()
            || !self.retired.is_empty()
        {
            return Ok(());
        }
        let attrs = c
            .build_ic_attributes()
            .push(
                AttributeName::InputStyle,
                InputStyle::PREEDIT_CALLBACKS | InputStyle::STATUS_NOTHING,
            )
            .push(AttributeName::ClientWindow, self.window)
            .push(AttributeName::FocusWindow, self.window)
            .build();
        c.create_ic(self.im, attrs)?;
        self.creating = Some(self.generation);
        Ok(())
    }
    fn event_mask(event: &KeyPressEvent) -> u32 {
        if event.response_type & 0x7f == x11rb::protocol::xproto::KEY_PRESS_EVENT {
            u32::from(x11rb::protocol::xproto::EventMask::KEY_PRESS)
        } else {
            u32::from(x11rb::protocol::xproto::EventMask::KEY_RELEASE)
        }
    }
    fn set_masks(&mut self, im: u16, ic: u16, forward: u32, synchronous: u32) {
        if self.im != 0 && self.im != im {
            return;
        }
        if ic == 0 {
            self.default_forward_mask = forward;
            self.default_synchronous_mask = synchronous;
            if !self.individual_mask {
                self.forward_mask = forward;
                self.synchronous_mask = synchronous;
            }
        } else if !self.retired.contains(&ic)
            && (self.ic == ic || (self.ic == 0 && self.creating == Some(self.generation)))
        {
            self.mask_ic = ic;
            self.individual_mask = true;
            self.forward_mask = forward;
            self.synchronous_mask = synchronous;
        }
    }
    fn forwards(&self, event: &KeyPressEvent) -> bool {
        self.forward_mask & Self::event_mask(event) != 0
    }
    fn forward_flags(&self, event: &KeyPressEvent) -> xim::ForwardEventFlag {
        if self.synchronous_mask & Self::event_mask(event) != 0 {
            xim::ForwardEventFlag::SYNCHRONOUS
        } else {
            xim::ForwardEventFlag::empty()
        }
    }
    fn acknowledge_sync(&mut self, im: u16, ic: u16) -> bool {
        if !self.synchronizing || !self.accepts(im, ic) {
            return false;
        }
        self.synchronizing = false;
        true
    }
    fn forward(
        &mut self, event: &KeyPressEvent,
        mut send: impl FnMut(u16, u16, xim::ForwardEventFlag, &KeyPressEvent) -> Result<(), ClientError>,
    ) -> Result<bool, ClientError> {
        if !self.enabled || self.im == 0 {
            return Ok(false);
        }
        if self.ic == 0 {
            if self.creating.is_none() && self.retired.is_empty() {
                return Ok(false);
            }
            self.deferred_keys.push_back(*event);
            return Ok(true);
        }
        if self.synchronizing {
            // Local-only releases must not overtake a press waiting for XIM.
            // flush_deferred replays them locally once preceding keys are acknowledged.
            self.deferred_keys.push_back(*event);
            return Ok(true);
        }
        if !self.forwards(event) {
            return Ok(false);
        }
        let flags = self.forward_flags(event);
        send(self.im, self.ic, flags, event)?;
        self.synchronizing = flags.contains(xim::ForwardEventFlag::SYNCHRONOUS);
        Ok(true)
    }
    fn flush_deferred(
        &mut self,
        mut send: impl FnMut(u16, u16, xim::ForwardEventFlag, &KeyPressEvent) -> Result<(), ClientError>,
    ) -> Result<(), ClientError> {
        while self.enabled && self.ic != 0 && !self.synchronizing {
            let Some(event) = self.deferred_keys.pop_front() else { break };
            if self.forwards(&event) {
                self.forward(&event, &mut send)?;
            } else {
                self.pending.push_back(NativeEvent::Key(event));
            }
        }
        Ok(())
    }
    fn emit(&mut self, input: Ime) {
        self.pending.push_back(NativeEvent::Input(Event::Ime(input)));
    }
    fn preedit(&mut self) {
        let byte = char_to_byte(&self.preedit, self.caret);
        self.emit(Ime::Preedit { text: self.preedit.clone(), cursor: Some((byte, byte)) });
    }
    fn set_spot<C: Client>(&self, client: &mut C) -> Result<(), ClientError> {
        let Some(config) = &self.configuration else { return Ok(()) };
        let attrs = client
            .build_ic_attributes()
            .nested_list(AttributeName::PreeditAttributes, |b| {
                b.push(
                    AttributeName::SpotLocation,
                    xim::Point {
                        x: config.position.x.clamp(i16::MIN as f64, i16::MAX as f64) as i16,
                        y: (config.position.y + config.size.height)
                            .clamp(i16::MIN as f64, i16::MAX as f64)
                            as i16,
                    },
                );
            })
            .build();
        client.set_ic_values(self.im, self.ic, attrs)
    }
}
impl<C: Client<XEvent = KeyPressEvent>> ClientHandler<C> for Handler {
    fn handle_connect(&mut self, c: &mut C) -> Result<(), ClientError> {
        // XIM negotiates a locale string; never call setlocale in a plugin host.
        let locale = std::env::var("LC_ALL")
            .ok()
            .filter(|s| !s.is_empty())
            .or_else(|| std::env::var("LC_CTYPE").ok().filter(|s| !s.is_empty()))
            .or_else(|| std::env::var("LANG").ok())
            .unwrap_or_else(|| "en_US.UTF-8".into());
        c.open(&locale)
    }
    fn handle_open(&mut self, c: &mut C, im: u16) -> Result<(), ClientError> {
        self.im = im;
        self.create_if_ready(c)
    }
    fn handle_create_ic(&mut self, c: &mut C, im: u16, ic: u16) -> Result<(), ClientError> {
        if !self.activate_created(im, ic) {
            return c.destroy_ic(im, ic);
        }
        self.set_spot(c)?;
        c.set_focus(im, ic)?;
        self.flush_deferred(|im, ic, flags, event| c.forward_event(im, ic, flags, event))
    }
    fn handle_sync_reply(&mut self, c: &mut C, im: u16, ic: u16) -> Result<(), ClientError> {
        if self.acknowledge_sync(im, ic) {
            self.flush_deferred(|im, ic, flags, event| c.forward_event(im, ic, flags, event))?;
        }
        Ok(())
    }
    fn handle_destroy_ic(&mut self, c: &mut C, im: u16, ic: u16) -> Result<(), ClientError> {
        if im == self.im {
            self.retired.retain(|old| *old != ic);
            self.create_if_ready(c)?;
        }
        Ok(())
    }
    fn handle_forward_event(
        &mut self, _: &mut C, im: u16, ic: u16, _: xim::ForwardEventFlag, event: KeyPressEvent,
    ) -> Result<(), ClientError> {
        if self.accepts(im, ic) {
            self.pending.push_back(NativeEvent::Key(event));
        }
        Ok(())
    }
    fn handle_commit(
        &mut self, _: &mut C, im: u16, ic: u16, text: &str,
    ) -> Result<(), ClientError> {
        if self.accepts(im, ic) {
            self.emit(Ime::Commit(text.into()));
            self.preedit.clear();
        }
        Ok(())
    }
    fn handle_preedit_start(&mut self, _: &mut C, im: u16, ic: u16) -> Result<(), ClientError> {
        if self.accepts(im, ic) {
            self.preedit.clear();
            self.caret = 0;
        }
        Ok(())
    }
    fn handle_preedit_done(&mut self, _: &mut C, im: u16, ic: u16) -> Result<(), ClientError> {
        if self.accepts(im, ic) {
            self.preedit.clear();
            self.caret = 0;
            self.preedit();
        }
        Ok(())
    }
    fn handle_preedit_draw(
        &mut self, _: &mut C, im: u16, ic: u16, caret: i32, first: i32, len: i32,
        _: xim::PreeditDrawStatus, text: &str, _: Vec<xim::Feedback>,
    ) -> Result<(), ClientError> {
        if !self.accepts(im, ic) {
            return Ok(());
        }
        replace_preedit(&mut self.preedit, first, len, text);
        self.caret = caret.max(0) as usize;
        if self.enabled {
            self.preedit();
        }
        Ok(())
    }
    fn handle_preedit_caret(
        &mut self, _: &mut C, im: u16, ic: u16, position: &mut i32, direction: xim::CaretDirection,
        _: xim::CaretStyle,
    ) -> Result<(), ClientError> {
        if !self.accepts(im, ic) {
            return Ok(());
        }
        self.caret = match direction {
            xim::CaretDirection::ForwardChar => self.caret.saturating_add(1),
            xim::CaretDirection::BackwardChar => self.caret.saturating_sub(1),
            xim::CaretDirection::AbsolutePosition => (*position).max(0) as usize,
            _ => self.caret,
        }
        .min(self.preedit.chars().count());
        *position = self.caret as i32;
        if self.enabled {
            self.preedit();
        }
        Ok(())
    }
    fn handle_set_event_mask(
        &mut self, _: &mut C, im: u16, ic: u16, forward: u32, synchronous: u32,
    ) -> Result<(), ClientError> {
        self.set_masks(im, ic, forward, synchronous);
        Ok(())
    }
    fn handle_disconnect(&mut self) {
        self.ic = 0;
        self.im = 0;
        self.preedit.clear();
        self.deferred_keys.clear();
        self.creating = None;
        self.synchronizing = false;
        self.retired.clear();
        self.generation = self.generation.wrapping_add(1);
        self.default_forward_mask = 0;
        self.default_synchronous_mask = 0;
        self.individual_mask = false;
        self.mask_ic = 0;
        self.forward_mask = 0;
        self.synchronous_mask = 0;
        self.emit(Ime::Disabled);
    }
}

fn replace_preedit(preedit: &mut String, first: i32, len: i32, text: &str) {
    let start = char_to_byte(preedit, first.max(0) as usize);
    let end = char_to_byte(preedit, first.max(0).saturating_add(len.max(0)) as usize);
    preedit.replace_range(start..end, text);
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn asynchronous_sync_acknowledgements_serialize_keys_and_reject_stale_contexts() {
        let mut handler = Handler {
            im: 3,
            ic: 7,
            enabled: true,
            forward_mask: u32::from(x11rb::protocol::xproto::EventMask::KEY_PRESS),
            synchronous_mask: u32::from(x11rb::protocol::xproto::EventMask::KEY_PRESS),
            ..Handler::default()
        };
        let mut sent = Vec::new();
        let mut send = |im, ic, flags, event: &KeyPressEvent| {
            sent.push((im, ic, flags, event.detail));
            Ok(())
        };
        let mut key = KeyPressEvent {
            response_type: x11rb::protocol::xproto::KEY_PRESS_EVENT,
            detail: 38,
            ..KeyPressEvent::default()
        };
        assert!(handler.forward(&key, &mut send).unwrap());
        key.detail = 39;
        assert!(handler.forward(&key, &mut send).unwrap());
        assert_eq!(handler.deferred_keys.len(), 1);
        assert!(handler.synchronizing);
        assert!(!handler.acknowledge_sync(3, 9));
        assert!(handler.synchronizing, "stale IC ACK cannot release outstanding request");
        assert!(handler.acknowledge_sync(3, 7));
        handler.flush_deferred(&mut send).unwrap();
        assert!(handler.synchronizing);
        key.response_type = x11rb::protocol::xproto::KEY_RELEASE_EVENT;
        assert!(handler.forward(&key, &mut send).unwrap(), "release waits for preceding press");
        assert!(handler.acknowledge_sync(3, 7));
        handler.flush_deferred(&mut send).unwrap();
        assert!(
            matches!(handler.pending.pop_front(), Some(NativeEvent::Key(event)) if event.response_type == x11rb::protocol::xproto::KEY_RELEASE_EVENT)
        );
        assert!(!handler.forward(&key, &mut send).unwrap(), "idle unrequested release stays local");
        assert_eq!(sent.len(), 2, "one queued key sent only after ACK");
        assert!(sent
            .iter()
            .all(|(_, _, flags, _)| flags.contains(xim::ForwardEventFlag::SYNCHRONOUS)));
        handler.begin_generation();
        handler.retired.clear(); // Destruction reply precedes any numeric ID reuse.
        handler.creating = Some(handler.generation);
        assert!(handler.activate_created(3, 9));
        handler.forward_mask = 1;
        handler.synchronous_mask = 1;
        key.response_type = x11rb::protocol::xproto::KEY_PRESS_EVENT;
        handler.forward(&key, |_, _, _, _| Ok(())).unwrap();
        assert!(!handler.acknowledge_sync(3, 7), "retired IC ACK cannot unlock new IC");
        assert!(handler.synchronizing);
        assert!(handler.acknowledge_sync(3, 9));
    }
    #[test]
    fn local_release_waits_for_both_synchronous_presses_without_being_sent_to_xim() {
        let mut handler = Handler {
            im: 3,
            ic: 7,
            enabled: true,
            forward_mask: u32::from(x11rb::protocol::xproto::EventMask::KEY_PRESS),
            synchronous_mask: u32::from(x11rb::protocol::xproto::EventMask::KEY_PRESS),
            ..Handler::default()
        };
        let press = |detail| KeyPressEvent {
            response_type: x11rb::protocol::xproto::KEY_PRESS_EVENT,
            detail,
            ..KeyPressEvent::default()
        };
        let first = press(38);
        let second = press(39);
        let release =
            KeyPressEvent { response_type: x11rb::protocol::xproto::KEY_RELEASE_EVENT, ..second };
        let mut sent = Vec::new();
        let mut send = |_: u16, _: u16, _: xim::ForwardEventFlag, event: &KeyPressEvent| {
            sent.push((event.response_type, event.detail));
            Ok(())
        };
        assert!(handler.forward(&first, &mut send).unwrap());
        assert!(handler.forward(&second, &mut send).unwrap());
        assert!(handler.forward(&release, &mut send).unwrap());
        assert_eq!(handler.deferred_keys.len(), 2);
        assert!(handler.pending.is_empty(), "release cannot reach the UI before the press");
        assert!(handler.acknowledge_sync(3, 7));
        handler.flush_deferred(&mut send).unwrap();
        assert_eq!(handler.deferred_keys.len(), 1);
        assert!(handler.pending.is_empty(), "second press still awaits XIM");
        // XIM returns the unconsumed press before acknowledging its request.
        handler.pending.push_back(NativeEvent::Key(second));
        assert!(handler.acknowledge_sync(3, 7));
        handler.flush_deferred(&mut send).unwrap();
        assert!(handler.deferred_keys.is_empty());
        assert!(!handler.synchronizing);
        assert_eq!(
            sent,
            vec![(first.response_type, first.detail), (second.response_type, second.detail)],
            "local-only release never enters the transport"
        );
        let delivered: Vec<_> = handler
            .pending
            .into_iter()
            .map(|event| match event {
                NativeEvent::Key(event) => (event.response_type, event.detail),
                NativeEvent::Input(_) => panic!("unexpected input event"),
            })
            .collect();
        assert_eq!(
            delivered,
            vec![
                (x11rb::protocol::xproto::KEY_PRESS_EVENT, 39),
                (x11rb::protocol::xproto::KEY_RELEASE_EVENT, 39),
            ]
        );
    }
    #[test]
    fn retired_context_and_pending_creation_cannot_route_old_field_callbacks() {
        let mut handler = Handler { im: 3, ic: 7, enabled: true, ..Handler::default() };
        assert!(handler.accepts(3, 7));
        assert_eq!(handler.begin_generation(), Some(7));
        assert!(!handler.accepts(3, 7));
        assert_eq!(handler.retired, vec![7]);
        handler.creating = Some(handler.generation);
        assert!(!handler.activate_created(3, 7), "no reuse before destruction acknowledgement");
        handler.retired.clear(); // DestroyIcReply is the protocol ordering barrier.
        handler.creating = Some(handler.generation);
        assert!(handler.activate_created(3, 9));
        assert!(handler.accepts(3, 9));
        assert!(!handler.accepts(3, 7));
        handler.enabled = false;
        assert!(!handler.accepts(3, 9), "late forwarded key or commit after blur");
    }
    #[test]
    fn late_create_reply_is_retired_when_focus_changes_during_creation() {
        let mut handler = Handler { im: 3, enabled: true, creating: Some(0), ..Handler::default() };
        handler.begin_generation();
        handler.set_masks(3, 7, 3, 3);
        assert!(!handler.individual_mask, "old-generation create cannot install masks");
        assert!(!handler.activate_created(8, 7));
        assert_eq!(handler.creating, Some(0), "foreign IM reply cannot consume our request");
        assert!(!handler.activate_created(3, 7));
        assert_eq!(handler.ic, 0);
        assert_eq!(handler.retired, vec![7]);
        assert!(!handler.accepts(3, 7));
        handler.set_masks(3, 7, 3, 3);
        assert!(!handler.individual_mask, "retired IC cannot install masks");
        handler.retired.clear();
        handler.creating = Some(handler.generation);
        handler.enabled = false;
        assert!(!handler.activate_created(3, 9), "blur during creation");
        assert!(!handler.accepts(3, 9));
    }
    #[test]
    fn xim_default_masks_apply_only_without_an_individual_context_mask() {
        let mut handler = Handler { im: 3, ic: 7, ..Handler::default() };
        handler.set_masks(3, 0, 1, 1);
        assert_eq!((handler.forward_mask, handler.synchronous_mask), (1, 1));
        handler.set_masks(3, 7, 3, 0);
        handler.set_masks(3, 0, 0, 0);
        assert_eq!((handler.forward_mask, handler.synchronous_mask), (3, 0));
        handler.set_masks(3, 9, 4, 4);
        handler.set_masks(8, 7, 4, 4);
        assert_eq!((handler.forward_mask, handler.synchronous_mask), (3, 0));
    }
    #[test]
    fn xim_forward_masks_leave_unrequested_releases_on_the_local_key_path() {
        let handler = Handler {
            forward_mask: u32::from(x11rb::protocol::xproto::EventMask::KEY_PRESS),
            synchronous_mask: u32::from(x11rb::protocol::xproto::EventMask::KEY_PRESS),
            ..Handler::default()
        };
        let mut key = KeyPressEvent {
            response_type: x11rb::protocol::xproto::KEY_PRESS_EVENT,
            ..KeyPressEvent::default()
        };
        assert!(handler.forwards(&key));
        assert!(handler.forward_flags(&key).contains(xim::ForwardEventFlag::SYNCHRONOUS));
        key.response_type = x11rb::protocol::xproto::KEY_RELEASE_EVENT;
        assert!(!handler.forwards(&key));
        assert!(handler.forward_flags(&key).is_empty());
    }
    #[test]
    fn xim_incremental_changes_are_character_ranges() {
        let mut text = "a😀漢z".to_owned();
        replace_preedit(&mut text, 1, 2, "日本");
        assert_eq!(text, "a日本z");
        assert_eq!(char_to_byte(&text, 3), 7);
        replace_preedit(&mut text, 1, 2, "");
        assert_eq!(text, "az");
        replace_preedit(&mut text, 100, 100, "é");
        assert_eq!(text, "azé");
    }
}
