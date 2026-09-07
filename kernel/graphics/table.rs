//! The kernel-wide table of live [`Surface`]s.
//!
//! A surface has one integer id, valid across every VM. A program creates a
//! surface and gets its id back; it can send that id to another process as
//! an ordinary number, and both processes then reach the same pixels. An
//! entry stays alive while any live process holds it — its creator, plus
//! anyone who has revived the id with `useSurface` — and is dropped once its
//! last holder is gone.
//!
//! The screen is [`SCREEN_ID`], created at boot with a permanent hold by
//! process 0 (the kernel) so it is never reclaimed. Resizing the screen is
//! how the window's logical resolution changes.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use tiny_skia::{Rect, Transform};

use super::Surface;
use crate::process::ProcessId;

/// A surface's kernel-wide id. Plain integers, so a handle crosses a VM
/// boundary as an ordinary JSON number.
pub type SurfaceId = u32;

/// The screen. Always present, never reclaimed.
pub const SCREEN_ID: SurfaceId = 0;

/// A ceiling on the total pixel memory every live surface holds at once, so
/// a program can't exhaust the heap through the kernel's table. Room for the
/// screen many times over.
const MAX_TOTAL_BYTES: usize = 64 * 1024 * 1024;

fn byte_size(width: u32, height: u32) -> usize {
    width as usize * height as usize * 4
}

struct Entry {
    surface: Surface,
    /// Every live process that holds this surface's id. The entry is dropped
    /// when this empties (unless it is the screen).
    holders: BTreeSet<ProcessId>,
}

/// Why a surface operation from a program couldn't be carried out. Each maps
/// to a thrown JS error at the binding boundary.
#[derive(Debug, PartialEq, Eq)]
pub enum SurfaceError {
    /// No live surface has this id.
    NoSuchSurface(SurfaceId),
    /// A surface can't be drawn onto itself.
    SelfBlit,
    /// The table is already holding its ceiling of pixel memory.
    OutOfMemory,
    /// The requested dimensions are zero or too large to allocate.
    BadSize,
}

/// The one table of surfaces the whole kernel shares. Held behind an `Rc` in
/// [`crate::runtime::Devices`], so every VM's `ely:graphics` bindings and the
/// frame loop address the same entries.
pub struct SurfaceTable {
    next_id: Cell<SurfaceId>,
    entries: RefCell<BTreeMap<SurfaceId, Entry>>,
    total_bytes: Cell<usize>,
}

impl SurfaceTable {
    /// A table holding only the screen, `width` by `height`, held forever by
    /// process 0.
    pub fn with_screen(width: u32, height: u32) -> SurfaceTable {
        let screen = Surface::new(width, height).expect("failed to allocate the screen surface");
        let mut entries = BTreeMap::new();
        entries.insert(
            SCREEN_ID,
            Entry {
                surface: screen,
                holders: BTreeSet::from([0]),
            },
        );
        SurfaceTable {
            next_id: Cell::new(SCREEN_ID + 1),
            entries: RefCell::new(entries),
            total_bytes: Cell::new(byte_size(width, height)),
        }
    }

    /// Creates a surface `width` by `height`, held by `owner`, and returns
    /// its id.
    pub fn create(
        &self,
        width: u32,
        height: u32,
        owner: ProcessId,
    ) -> Result<SurfaceId, SurfaceError> {
        let bytes = byte_size(width, height);
        if self.total_bytes.get().saturating_add(bytes) > MAX_TOTAL_BYTES {
            return Err(SurfaceError::OutOfMemory);
        }
        let surface = Surface::new(width, height).ok_or(SurfaceError::BadSize)?;

        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1).max(SCREEN_ID + 1));
        self.entries.borrow_mut().insert(
            id,
            Entry {
                surface,
                holders: BTreeSet::from([owner]),
            },
        );
        self.total_bytes.set(self.total_bytes.get() + bytes);
        Ok(id)
    }

    /// Records `pid` as a holder of `id`. Returns whether `id` names a live
    /// surface — `useSurface` throws when it doesn't.
    pub fn acquire(&self, id: SurfaceId, pid: ProcessId) -> bool {
        match self.entries.borrow_mut().get_mut(&id) {
            Some(entry) => {
                entry.holders.insert(pid);
                true
            }
            None => false,
        }
    }

    /// Drops `pid`'s hold on `id`. The surface goes away once its last holder
    /// does, unless it is the screen. A no-op for an id `pid` never held.
    pub fn release(&self, id: SurfaceId, pid: ProcessId) {
        let mut entries = self.entries.borrow_mut();
        let Some(entry) = entries.get_mut(&id) else {
            return;
        };
        entry.holders.remove(&pid);
        if id != SCREEN_ID && entry.holders.is_empty() {
            let bytes = byte_size(entry.surface.width(), entry.surface.height());
            entries.remove(&id);
            self.total_bytes
                .set(self.total_bytes.get().saturating_sub(bytes));
        }
    }

    /// Drops every hold `pid` had. Called when a process is reaped, so a
    /// surface only its dead creator held is reclaimed, while one another
    /// live process was sent survives.
    pub fn release_all(&self, pid: ProcessId) {
        let ids: Vec<SurfaceId> = self.entries.borrow().keys().copied().collect();
        for id in ids {
            self.release(id, pid);
        }
    }

    /// This surface's current dimensions, or `None` if the id is dead.
    pub fn dimensions(&self, id: SurfaceId) -> Option<(u32, u32)> {
        self.entries
            .borrow()
            .get(&id)
            .map(|entry| (entry.surface.width(), entry.surface.height()))
    }

    /// Resizes `id`'s surface. Contents stay at the top-left; the transform
    /// and clip stacks are emptied. See [`Surface::resize`].
    pub fn resize(&self, id: SurfaceId, width: u32, height: u32) -> Result<(), SurfaceError> {
        let mut entries = self.entries.borrow_mut();
        let entry = entries
            .get_mut(&id)
            .ok_or(SurfaceError::NoSuchSurface(id))?;
        let old_bytes = byte_size(entry.surface.width(), entry.surface.height());
        let new_bytes = byte_size(width, height);
        if self.total_bytes.get() - old_bytes + new_bytes > MAX_TOTAL_BYTES {
            return Err(SurfaceError::OutOfMemory);
        }
        entry
            .surface
            .resize(width, height)
            .ok_or(SurfaceError::BadSize)?;
        self.total_bytes
            .set(self.total_bytes.get() - old_bytes + new_bytes);
        Ok(())
    }

    /// Runs `f` against `id`'s surface for a drawing operation. `Err` — a
    /// thrown error at the binding — when the id is dead.
    pub fn with_mut<R>(
        &self,
        id: SurfaceId,
        f: impl FnOnce(&mut Surface) -> R,
    ) -> Result<R, SurfaceError> {
        self.entries
            .borrow_mut()
            .get_mut(&id)
            .map(|entry| f(&mut entry.surface))
            .ok_or(SurfaceError::NoSuchSurface(id))
    }

    /// Runs `f` against `id`'s surface without mutating it — for
    /// presentation. `None` only if the id is dead, which never happens for
    /// the screen.
    pub fn with<R>(&self, id: SurfaceId, f: impl FnOnce(&Surface) -> R) -> Option<R> {
        self.entries
            .borrow()
            .get(&id)
            .map(|entry| f(&entry.surface))
    }

    /// Draws `src` whole onto `dst` with its top-left corner at `(x, y)`.
    pub fn draw_surface_whole(
        &self,
        dst: SurfaceId,
        src: SurfaceId,
        x: f32,
        y: f32,
    ) -> Result<(), SurfaceError> {
        self.blit(dst, src, |dst_surface, image| {
            dst_surface.draw_image(image, false, x, y)
        })
    }

    /// Draws `source` out of `src` onto `dst` under `transform` — the crop,
    /// flip, resize and turn form.
    pub fn draw_surface_transformed(
        &self,
        dst: SurfaceId,
        src: SurfaceId,
        source: Rect,
        transform: Transform,
    ) -> Result<(), SurfaceError> {
        self.blit(dst, src, |dst_surface, image| {
            dst_surface.draw_image_transformed(image, false, source, transform)
        })
    }

    /// Shared body of the two `draw_surface_*` forms: lifts `dst` out of the
    /// map so `src` can be borrowed alongside it, runs `draw`, and puts `dst`
    /// back. Refuses a surface drawn onto itself.
    fn blit(
        &self,
        dst: SurfaceId,
        src: SurfaceId,
        draw: impl FnOnce(&mut Surface, &tiny_skia::Pixmap),
    ) -> Result<(), SurfaceError> {
        if dst == src {
            return Err(SurfaceError::SelfBlit);
        }
        let mut entries = self.entries.borrow_mut();
        let mut dst_entry = entries
            .remove(&dst)
            .ok_or(SurfaceError::NoSuchSurface(dst))?;
        let result = match entries.get(&src) {
            Some(src_entry) => {
                draw(&mut dst_entry.surface, src_entry.surface.pixmap());
                Ok(())
            }
            None => Err(SurfaceError::NoSuchSurface(src)),
        };
        entries.insert(dst, dst_entry);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> SurfaceTable {
        SurfaceTable::with_screen(64, 32)
    }

    #[test]
    fn the_screen_is_present_from_the_start_and_never_reclaimed() {
        let table = table();
        assert_eq!(table.dimensions(SCREEN_ID), Some((64, 32)));
        // Even with no holders left, the screen stays.
        table.release(SCREEN_ID, 0);
        assert_eq!(table.dimensions(SCREEN_ID), Some((64, 32)));
    }

    #[test]
    fn a_created_surface_gets_a_fresh_id_past_the_screen() {
        let table = table();
        let a = table.create(10, 10, 1).unwrap();
        let b = table.create(10, 10, 1).unwrap();
        assert_ne!(a, SCREEN_ID);
        assert_ne!(a, b);
    }

    #[test]
    fn a_surface_outlives_its_creator_while_another_process_holds_it() {
        let table = table();
        let id = table.create(10, 10, 1).unwrap();
        assert!(table.acquire(id, 2));
        // The creator dies.
        table.release_all(1);
        assert_eq!(
            table.dimensions(id),
            Some((10, 10)),
            "process 2 still holds it"
        );
        // The last holder dies.
        table.release_all(2);
        assert_eq!(table.dimensions(id), None);
    }

    #[test]
    fn destroying_a_surface_only_drops_the_caller_s_hold() {
        let table = table();
        let id = table.create(10, 10, 1).unwrap();
        table.acquire(id, 2);
        table.release(id, 2); // process 2's explicit destroySurface
        assert_eq!(
            table.dimensions(id),
            Some((10, 10)),
            "creator still holds it"
        );
        table.release(id, 1);
        assert_eq!(table.dimensions(id), None);
    }

    #[test]
    fn reviving_a_dead_id_reports_it_missing() {
        let table = table();
        let id = table.create(10, 10, 1).unwrap();
        table.release_all(1);
        assert!(!table.acquire(id, 2));
    }

    #[test]
    fn drawing_a_surface_onto_itself_is_refused() {
        let table = table();
        let id = table.create(10, 10, 1).unwrap();
        assert_eq!(
            table.draw_surface_whole(id, id, 0.0, 0.0),
            Err(SurfaceError::SelfBlit)
        );
    }

    #[test]
    fn a_blit_between_two_surfaces_copies_pixels_across() {
        let table = table();
        let src = table.create(8, 8, 1).unwrap();
        let dst = table.create(16, 16, 1).unwrap();
        table
            .with_mut(src, |s| s.clear(super::super::Color::Amber400))
            .unwrap();
        table.draw_surface_whole(dst, src, 2.0, 2.0).unwrap();
        let hit = table
            .with(dst, |s| {
                s.pixmap().pixels()[(4 * 16 + 4) as usize].demultiply()
            })
            .unwrap();
        assert_eq!(hit.red(), (super::super::Color::Amber400.hex() >> 16) as u8);
    }

    #[test]
    fn the_memory_ceiling_rejects_an_oversized_surface() {
        let table = table();
        // Well past MAX_TOTAL_BYTES on its own.
        assert_eq!(
            table.create(20_000, 20_000, 1),
            Err(SurfaceError::OutOfMemory)
        );
    }

    #[test]
    fn a_resized_surface_reports_its_new_dimensions_to_every_holder() {
        let table = table();
        let id = table.create(10, 10, 1).unwrap();
        table.acquire(id, 2);
        table.resize(id, 40, 20).unwrap();
        assert_eq!(table.dimensions(id), Some((40, 20)));
    }
}
