// Ambient globals provided by the Elysium Rust host to every program's
// isolated JS VM. Keep this in sync with the bindings registered in
// src/runtime.rs.

/** Writes a line to the host's stdout. */
declare function print(...message: any): void;
