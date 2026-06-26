//! [`Property<V>`]: the server-side handle that publishes a [`ViewModel`] into
//! its [`SeqlockBytes`] cell, and [`PropertyReader<V>`]: the clone-able pump-side
//! reader of that cell.
//!
//! A `Property` is a cheap (`Arc`-backed) handle around one latest-value cell.
//! The RT control task calls [`Property::set`] once per cycle (allocation-free,
//! never blocks). It is the **sole writer**: `Property` is move-only (not
//! `Clone`), so the seqlock's single-producer invariant is enforced at the type
//! level — there is no way to obtain a second writer for the same cell.
//!
//! Reads go through [`PropertyReader<V>`], obtained via [`Property::reader`].
//! The reader *is* `Clone`-able: a seqlock tolerates any number of concurrent
//! readers, so the non-RT pump may hand clones to multiple consumers. A reader
//! calls [`snapshot`](PropertyReader::snapshot) /
//! [`snapshot_into`](PropertyReader::snapshot_into) to reconstruct the typed
//! ViewModel for JSON encoding off the RT path.
//!
//! # Image-byte soundness
//!
//! [`set`](Property::set) lowers the ViewModel to its `#[repr(C)] Copy` image
//! and views that image as a `&[u8]` to hand to the cell. The image may carry
//! **padding bytes** (e.g. a `bool` followed by an `f64`) that are not
//! explicitly initialised; viewing them as `u8` and copying them through the
//! seqlock is sound here because the bytes are copied wholesale and are only
//! ever interpreted back as the *same* `Image` type — never field-decoded from
//! the raw bytes. This is the documented padding caveat of byte-image seqlocks;
//! a padding-free `#[repr(C)]` layout (integers/floats in descending alignment,
//! which the derive produces) keeps it moot in practice.
//!
//! The reverse direction reconstructs the typed image **only after** the cell
//! confirms a tear-free read, so the bytes are always a value the producer
//! actually wrote — valid enum discriminants, no UB. The reconstruction uses
//! [`core::ptr::read_unaligned`] because the cell's backing buffer is a
//! `Vec<u8>` (alignment 1), not necessarily aligned for `V::Image`.

// `set` views a `#[repr(C)] Copy` image as bytes and `snapshot_into`
// reconstructs the image from tear-free bytes; both need `unsafe`, which the
// crate otherwise forbids via `#![deny(unsafe_code)]`. Each block is justified
// inline (see the module "Image-byte soundness" note).
#![allow(unsafe_code)]

use std::marker::PhantomData;
use std::sync::Arc;

use crate::cell::SeqlockBytes;
use crate::viewmodel::ViewModel;

/// The sole, move-only writer that publishes a [`ViewModel`] into one
/// latest-value seqlock cell.
///
/// `Property` is intentionally **not** `Clone`: the seqlock is sound with many
/// readers but is undefined behaviour with two concurrent writers, so the only
/// writer handle is move-only and unique by construction. Obtain clone-able
/// reader handles for the non-RT pump via [`reader`](Self::reader).
pub struct Property<V: ViewModel> {
    cell: Arc<SeqlockBytes>,
    _marker: PhantomData<fn() -> V>,
}

impl<V: ViewModel> Default for Property<V> {
    fn default() -> Self {
        Self::new()
    }
}

impl<V: ViewModel> Property<V> {
    /// Create a property with a freshly allocated cell sized to `V::IMAGE_SIZE`.
    /// This is the only allocation on the property's lifecycle; neither
    /// [`set`](Self::set) nor [`PropertyReader::snapshot_into`] allocates.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cell: Arc::new(SeqlockBytes::with_len(V::IMAGE_SIZE)),
            _marker: PhantomData,
        }
    }

    /// Obtain a clone-able reader handle for the non-RT pump.
    ///
    /// A seqlock tolerates any number of concurrent readers, so the returned
    /// [`PropertyReader`] (and its clones) may read the cell from any thread
    /// while this `Property` drives the single writer.
    #[must_use]
    pub fn reader(&self) -> PropertyReader<V> {
        PropertyReader {
            cell: Arc::clone(&self.cell),
            _marker: PhantomData,
        }
    }

    /// Publish `vm` as the latest value. Runs on the RT path: lowers `vm` to its
    /// image on the stack and copies the image bytes into the cell. No heap
    /// allocation, never blocks.
    pub fn set(&self, vm: &V) {
        let image = vm.to_image();
        // SAFETY: `image` is a live `#[repr(C)] Copy` value on the stack; a
        // `&[u8]` over `size_of::<V::Image>()` bytes from its address is in
        // bounds. Padding bytes may be read here — see the module "Image-byte
        // soundness" note; they round-trip as opaque bytes only.
        let bytes = unsafe {
            core::slice::from_raw_parts(
                core::ptr::addr_of!(image).cast::<u8>(),
                core::mem::size_of::<V::Image>(),
            )
        };
        self.cell.write(bytes);
    }
}

/// A clone-able reader of the [`ViewModel`] published by a [`Property`].
///
/// Obtained via [`Property::reader`]. Reading concurrently from multiple
/// `PropertyReader` clones is sound: a seqlock supports any number of readers
/// (only multiple *writers* would be undefined behaviour, which the move-only
/// [`Property`] prevents). Each clone shares the same underlying cell.
pub struct PropertyReader<V: ViewModel> {
    cell: Arc<SeqlockBytes>,
    _marker: PhantomData<fn() -> V>,
}

impl<V: ViewModel> Clone for PropertyReader<V> {
    fn clone(&self) -> Self {
        Self {
            cell: Arc::clone(&self.cell),
            _marker: PhantomData,
        }
    }
}

impl<V: ViewModel> PropertyReader<V> {
    /// Reconstruct the latest tear-free ViewModel into a caller-owned byte
    /// buffer (reused across calls; alloc-free once warm). Returns `None` if the
    /// property was never [`set`](Property::set) or the cell stayed torn.
    ///
    /// Runs off the RT path (the pump).
    pub fn snapshot_into(&self, buf: &mut Vec<u8>) -> Option<V> {
        if !self.cell.read_into(buf) {
            return None;
        }
        debug_assert_eq!(buf.len(), V::IMAGE_SIZE);
        // SAFETY: `read_into` returned `true`, so `buf` holds exactly the
        // `IMAGE_SIZE` bytes of one image the producer wrote, untorn — a valid
        // `V::Image` bit pattern. `read_unaligned` tolerates the `Vec<u8>`
        // buffer's alignment-1 storage.
        let image = unsafe { core::ptr::read_unaligned(buf.as_ptr().cast::<V::Image>()) };
        Some(V::from_image(&image))
    }

    /// Convenience snapshot that allocates a fresh buffer. Intended for tests
    /// and one-off reads; the pump uses [`snapshot_into`](Self::snapshot_into).
    #[must_use]
    pub fn snapshot(&self) -> Option<V> {
        let mut buf = Vec::new();
        self.snapshot_into(&mut buf)
    }
}
