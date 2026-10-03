use std::cell::LazyCell;
use std::ffi::{c_void, CStr};
use std::ops::Deref;
use std::ptr::NonNull;
use windows_core::Error;
use windows_sys::Win32::Foundation::FreeLibrary;
use windows_sys::Win32::System::LibraryLoader::{
    GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
};

/// # Safety
///
/// Implementations must ensure that the module with name `MODULE_NAME` is safe to load.
pub unsafe trait Module: Clone + Sized {
    const MODULE_NAME: &'static CStr;
    fn load(library: &RawLibrary) -> Self;
}

pub struct LibraryModule<M> {
    _library: RawLibrary,
    module: M,
}

pub type LazyLibraryModule<M> = LazyCell<Option<LibraryModule<M>>>;

impl<M: Module> LibraryModule<M> {
    pub fn load() -> Result<Self, Error> {
        let library = unsafe { RawLibrary::load(M::MODULE_NAME)? };
        Ok(Self { module: M::load(&library), _library: library })
    }

    pub fn lazy() -> LazyLibraryModule<M> {
        LazyCell::new(|| match Self::load() {
            Ok(module) => Some(module),
            Err(err) => {
                crate::warn!(
                    "Error loading module '{}': {}",
                    M::MODULE_NAME.to_string_lossy(),
                    err
                );

                None
            }
        })
    }
}

impl<M: Module> Clone for LibraryModule<M> {
    fn clone(&self) -> Self {
        let library = unsafe { RawLibrary::load(M::MODULE_NAME) };

        // PANIC: This should not be able to happen, since we already loaded it once and it's still loaded in Clone
        let library = match library {
            Ok(library) => library,
            Err(e) => unreachable!("Failed to load module: {}", e),
        };

        Self { _library: library, module: self.module.clone() }
    }
}

impl<M> Deref for LibraryModule<M> {
    type Target = M;
    fn deref(&self) -> &M {
        &self.module
    }
}

pub struct RawLibrary(NonNull<c_void>);

impl RawLibrary {
    pub unsafe fn load(module_name: &CStr) -> Result<Self, Error> {
        // MOOSE: wide-char API, and only look in System32 so a DLL planted next to
        // the host executable or in the current directory can never be picked up.
        let wide: Vec<u16> =
            module_name.to_bytes().iter().map(|&b| u16::from(b)).chain(Some(0)).collect();
        let library = unsafe {
            LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)
        };
        let Some(library) = NonNull::new(library) else { return Err(Error::from_thread()) };

        Ok(Self(library))
    }

    /// # Safety
    ///
    /// T *must* be a function pointer type, and must match the given `name`.
    pub unsafe fn get<T: Copy + Sized>(&self, name: &CStr) -> Option<T> {
        let addr = unsafe { GetProcAddress(self.0.as_ptr(), name.as_ptr().cast()) }?;

        Some(core::mem::transmute_copy(&addr))
    }
}

impl Drop for RawLibrary {
    fn drop(&mut self) {
        unsafe { FreeLibrary(self.0.as_ptr()) };
    }
}
