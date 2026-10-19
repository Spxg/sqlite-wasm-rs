#![cfg(feature = "threadsafe")]

use libsqlite3_sys as _;
use rsqlite_vfs::memvfs::{MemVfsContext, MemVfsContextError};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

std::thread_local! {
    static ARMED: Cell<bool> = const { Cell::new(false) };
    static REENTRY_REFUSED: Cell<bool> = const { Cell::new(false) };
}

struct Allocator;

// SAFETY: allocations and deallocations delegate to `System` with unchanged pointers and layouts.
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies a valid allocation layout.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        if ARMED
            .try_with(|armed| armed.replace(false))
            .unwrap_or(false)
        {
            let refused = matches!(
                MemVfsContext::try_new(),
                Err(MemVfsContextError::InitializationInProgress)
            );
            let _ = REENTRY_REFUSED.try_with(|seen| seen.set(refused));
        }
        // SAFETY: the caller returns the unchanged allocation and layout to `System`.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

#[test]
fn expired_context_cleanup_allows_allocator_reentry() {
    let context = MemVfsContext::try_new().unwrap();
    drop(context);
    ARMED.with(|armed| armed.set(true));
    let context = MemVfsContext::try_new().unwrap();
    assert!(REENTRY_REFUSED.with(Cell::get));
    drop(context);
}
