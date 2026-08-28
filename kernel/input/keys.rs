//! `Key`: one physical key on the keyboard, independent of the active
//! keyboard layout — the same variant names and meaning as winit's
//! [`KeyCode`](winit::keyboard::KeyCode), which this module mirrors 1:1 and
//! translates into on the way in from a raw window event. The `Key` enum,
//! `Key::COUNT`, and `Key::from_key_code` below are generated from
//! `build/keys.rs`'s `KEYS` table — the one source that also feeds the
//! `ely:input` TS module's `Key` constants and their `elysium.d.ts` types —
//! so the numeric id a program passes across the `ely:input` boundary can't
//! drift from this enum's discriminants.

include!(concat!(env!("OUT_DIR"), "/keys.rs"));

impl Key {
    /// Converts a numeric id crossing the `ely:input` boundary from JS back
    /// into a `Key`, or `None` if it's out of range.
    pub fn from_id(id: u16) -> Option<Key> {
        if (id as usize) < Self::COUNT {
            // SAFETY: `Key` is `#[repr(u16)]` with dense discriminants
            // `0..COUNT`, just checked `id` falls in that range.
            Some(unsafe { std::mem::transmute::<u16, Key>(id) })
        } else {
            None
        }
    }
}
