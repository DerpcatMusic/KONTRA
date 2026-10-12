//! IMM32 composition; consumes result messages so DefWindowProc cannot commit again.
use crate::{Ime, ImeConfiguration};
use std::{
    cell::{Cell, RefCell},
    ptr,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows_sys::Win32::{
    Foundation::{HWND, POINT, RECT},
    UI::{Input::Ime::*, WindowsAndMessaging::*},
};

#[derive(Default)]
pub(crate) struct NativeIme {
    configuration: RefCell<Option<ImeConfiguration>>,
    enabled: Cell<Option<bool>>,
    configured: Cell<bool>,
    cancel_pending: Cell<bool>,
    pub composing: Cell<bool>,
    applying: Cell<bool>,
}
struct Context {
    hwnd: HWND,
    himc: HIMC,
}
impl Context {
    fn get(hwnd: HWND) -> Option<Self> {
        let himc = unsafe { ImmGetContext(hwnd) };
        (!himc.is_null()).then_some(Self { hwnd, himc })
    }
    fn text(&self, kind: u32) -> Option<String> {
        let len = unsafe { ImmGetCompositionStringW(self.himc, kind, ptr::null_mut(), 0) };
        if len < 0 || len % 2 != 0 {
            return None;
        }
        let mut units = vec![0u16; len as usize / 2];
        let copied = unsafe {
            ImmGetCompositionStringW(self.himc, kind, units.as_mut_ptr().cast(), len as u32)
        };
        if copied < 0 || copied > len {
            return None;
        }
        units.truncate(copied as usize / 2);
        Some(String::from_utf16_lossy(&units))
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            ImmReleaseContext(self.hwnd, self.himc);
        }
    }
}

impl NativeIme {
    pub fn configure(&self, hwnd: HWND, config: Option<ImeConfiguration>) {
        let config = config.filter(ImeConfiguration::valid);
        if self.configured.replace(true) && *self.configuration.borrow() == config {
            return;
        }
        let cancelled = crate::ime::composition_cancelled(
            &self.configuration.borrow(),
            &config,
            self.composing.get(),
        );
        let switched =
            self.configuration.borrow().as_ref().map(|c| &c.id) != config.as_ref().map(|c| &c.id);
        if switched || cancelled {
            self.cancel_pending.set(true);
        }
        *self.configuration.borrow_mut() = config;
        // Native calls can reenter. The RefCell borrow has ended before the first call.
        unsafe {
            PostMessageW(hwnd, super::window::BV_IME_CONFIGURE, 0, 0);
        }
    }
    pub fn accepts(&self, hwnd: HWND) -> bool {
        self.enabled.get() == Some(true)
            && self.configuration.borrow().is_some()
            && !self.cancel_pending.get()
            && !self.applying.get()
            && unsafe { GetFocus() == hwnd }
    }
    pub fn apply(&self, hwnd: HWND) -> Option<Ime> {
        let on = self.configuration.borrow().is_some();
        let changed = self.enabled.replace(Some(on)) != Some(on);
        let cancel = self.cancel_pending.replace(false);
        if changed || cancel {
            self.applying.set(true);
            unsafe {
                if (!on || cancel) && GetFocus() == hwnd {
                    if let Some(ctx) = Context::get(hwnd) {
                        ImmNotifyIME(ctx.himc, NI_COMPOSITIONSTR, CPS_CANCEL, 0);
                    }
                }
                if changed {
                    ImmAssociateContextEx(hwnd, ptr::null_mut(), if on { IACE_DEFAULT } else { 0 });
                }
            }
            self.composing.set(false);
            self.applying.set(false);
        }
        self.position(hwnd);
        changed.then_some(if on { Ime::Enabled } else { Ime::Disabled })
    }
    pub fn position(&self, hwnd: HWND) {
        let config = self.configuration.borrow().clone();
        let Some(config) = config else {
            return;
        };
        let Some(ctx) = Context::get(hwnd) else {
            return;
        };
        let (x, y, right, bottom) = crate::ime::candidate_rect(&config);
        unsafe {
            ImmSetCompositionWindow(
                ctx.himc,
                &COMPOSITIONFORM {
                    dwStyle: CFS_POINT,
                    ptCurrentPos: POINT { x, y },
                    rcArea: RECT::default(),
                },
            );
            ImmSetCandidateWindow(
                ctx.himc,
                &CANDIDATEFORM {
                    dwIndex: 0,
                    dwStyle: CFS_EXCLUDE,
                    ptCurrentPos: POINT { x, y: bottom },
                    rcArea: RECT { left: x, top: y, right, bottom },
                },
            );
        }
    }
    pub fn composition(&self, hwnd: HWND, flags: u32) -> Vec<Ime> {
        if !self.accepts(hwnd) {
            return Vec::new();
        }
        let Some(ctx) = Context::get(hwnd) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        if flags & GCS_RESULTSTR != 0 {
            if let Some(text) = ctx.text(GCS_RESULTSTR) {
                events.push(Ime::Commit(text));
            }
            self.composing.set(false);
        }
        if flags & GCS_COMPSTR != 0 {
            if let Some(text) = ctx.text(GCS_COMPSTR) {
                let pos = unsafe {
                    ImmGetCompositionStringW(ctx.himc, GCS_CURSORPOS, ptr::null_mut(), 0)
                };
                let cursor = (pos >= 0).then(|| {
                    let byte = crate::ime::utf16_to_byte(&text, pos as usize);
                    (byte, byte)
                });
                self.composing.set(!text.is_empty());
                events.push(Ime::Preedit { text, cursor });
            }
        } else if flags == 0 {
            self.composing.set(false);
            events.push(Ime::Preedit { text: String::new(), cursor: None });
        }
        events
    }
    pub fn reconversion(&self, hwnd: HWND, lparam: isize) -> Option<isize> {
        if !self.accepts(hwnd) {
            return None;
        }
        let config = self.configuration.borrow().clone()?;
        let utf16: Vec<_> = config.text.encode_utf16().collect();
        let bytes = std::mem::size_of::<RECONVERTSTRING>() + (utf16.len() + 1) * 2;
        if lparam == 0 {
            return Some(bytes as isize);
        }
        let ptr = lparam as *mut RECONVERTSTRING;
        unsafe {
            if (*ptr).dwSize < bytes as u32 {
                return Some(0);
            }
            let start = config.text[..config.selection.start].encode_utf16().count() as u32;
            let length = config.text[config.selection.clone()].encode_utf16().count() as u32;
            let marked = config.marked.as_ref().unwrap_or(&config.selection);
            let comp_start = config.text[..marked.start].encode_utf16().count() as u32;
            let comp_length = config.text[marked.clone()].encode_utf16().count() as u32;
            *ptr = RECONVERTSTRING {
                dwSize: bytes as u32,
                dwVersion: 0,
                dwStrLen: utf16.len() as u32,
                dwStrOffset: std::mem::size_of::<RECONVERTSTRING>() as u32,
                dwCompStrLen: comp_length,
                dwCompStrOffset: comp_start * 2,
                dwTargetStrLen: length,
                dwTargetStrOffset: start * 2,
            };
            let text_ptr =
                ptr.cast::<u8>().add(std::mem::size_of::<RECONVERTSTRING>()).cast::<u16>();
            ptr::copy_nonoverlapping(utf16.as_ptr(), text_ptr, utf16.len());
            text_ptr.add(utf16.len()).write(0);
        }
        Some(bytes as isize)
    }
}
