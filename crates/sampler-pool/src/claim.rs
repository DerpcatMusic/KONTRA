#![allow(unsafe_code)]
use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

/// Per-unit "held" flags.
pub struct Claims(Box<[AtomicBool]>);

impl Claims {
    pub fn new(units: usize) -> Self {
        Self((0..units).map(|_| AtomicBool::new(false)).collect())
    }
}

/// Exclusive access to one unit of a [`Slab`] or [`Disjoint`]; released on drop.
pub struct Claim<'a, T> {
    ptr: *mut T,
    len: usize,
    flag: &'a AtomicBool,
    _borrow: PhantomData<&'a mut [T]>,
}

impl<T> Deref for Claim<'_, T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        // SAFETY: the held flag makes this claim the only access to the unit.
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}
impl<T> DerefMut for Claim<'_, T> {
    fn deref_mut(&mut self) -> &mut [T] {
        // SAFETY: as above; `&mut self` makes the borrow unique.
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}
impl<T> Drop for Claim<'_, T> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}
// SAFETY: a claim is unique access to `[T]`, like `&mut [T]`.
unsafe impl<T: Send> Send for Claim<'_, T> {}

fn take<'a, T>(ptr: *mut T, len: usize, unit: usize, flags: &'a [AtomicBool], i: usize) -> Claim<'a, T> {
    let flag = &flags[i];
    assert!(
        !flag.swap(true, Ordering::Acquire),
        "unit {i} is already claimed"
    );
    let start = i * unit;
    let end = (start + unit).min(len);
    Claim {
        // SAFETY: `start <= len` because flag `i` exists, so the unit is in bounds.
        ptr: unsafe { ptr.add(start) },
        len: end - start,
        flag,
        _borrow: PhantomData,
    }
}

/// Owned storage whose fixed-size units can be claimed through `&self`.
pub struct Slab<T> {
    items: Box<[T]>,
    flags: Claims,
    unit: usize,
    /// Stands in for the flag of the empty claims of empty storage.
    idle: AtomicBool,
}

impl<T> Slab<T> {
    /// `unit` items per claim (the last unit may be shorter).
    pub fn new(items: Box<[T]>, unit: usize) -> Self {
        let unit = unit.max(1);
        let flags = Claims::new(items.len().div_ceil(unit));
        Self { items, flags, unit, idle: AtomicBool::new(false) }
    }
    /// Take the storage back.
    pub fn into_items(self) -> Box<[T]> {
        self.items
    }
    /// The whole storage; exclusive, so no claim can be live.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.items
    }
    pub fn as_slice(&mut self) -> &[T] {
        &self.items
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    /// Unit `i`. Panics when `i` is out of range or the unit is held.
    pub fn claim(&self, i: usize) -> Claim<'_, T> {
        if self.items.is_empty() {
            // Nothing to guard: units of empty storage are empty, for any `i`.
            return Claim {
                ptr: std::ptr::NonNull::dangling().as_ptr(),
                len: 0,
                flag: &self.idle,
                _borrow: PhantomData,
            };
        }
        take(self.items.as_ptr().cast_mut(), self.items.len(), self.unit, &self.flags.0, i)
    }
}
// SAFETY: `&Slab` only yields `&mut T` through claims, which never overlap.
unsafe impl<T: Send> Sync for Slab<T> {}

/// A borrowed slice whose units can be claimed through `&self`. The flags
/// belong to the caller so a `Disjoint` is free to build every block.
pub struct Disjoint<'a, T> {
    ptr: *mut T,
    len: usize,
    unit: usize,
    flags: &'a [AtomicBool],
    _borrow: PhantomData<&'a mut [T]>,
}

impl<'a, T> Disjoint<'a, T> {
    /// `flags` needs `items.len().div_ceil(unit)` units.
    pub fn new(items: &'a mut [T], unit: usize, flags: &'a Claims) -> Self {
        let unit = unit.max(1);
        assert!(flags.0.len() >= items.len().div_ceil(unit), "too few claim flags");
        Self { ptr: items.as_mut_ptr(), len: items.len(), unit, flags: &flags.0, _borrow: PhantomData }
    }
    pub fn claim(&self, i: usize) -> Claim<'_, T> {
        take(self.ptr, self.len, self.unit, self.flags, i)
    }
}
// SAFETY: as for `Slab`.
unsafe impl<T: Send> Sync for Disjoint<'_, T> {}
unsafe impl<T: Send> Send for Disjoint<'_, T> {}
