//! File and directory I/O backing `ely:filesystem`'s native bindings.
//! Unlike `kernel/image.rs`, no per-VM resource table is needed here — every
//! operation (read, write, stat, list, create, remove) is one-shot, so
//! `bootstrap_filesystem_bindings` only ever captures a cloned
//! `userland_root`.
//!
//! Symlinks are made fully invisible rather than transparently followed:
//! [`resolve_userland_path`] and [`resolve_userland_path_for_write`] walk a
//! requested path component by component from the userland root, checking
//! `std::fs::symlink_metadata` at every step, and treat any symlink
//! component as though the path didn't exist at all. Both are `pub`: they
//! are the one place userland-path sandboxing is implemented, and every
//! other native surface that resolves a virtual path — `ely:image`'s
//! `loadImage`, the process manager's program loading — goes through them
//! rather than rolling its own `canonicalize`-based check.
//!
//! Every thrown message is tagged with a machine-readable prefix
//! (`"NOT_FOUND: ..."`, `"IS_A_DIRECTORY: ..."`, ...) that `ely:filesystem`
//! parses to pick which of its specific, shared error classes
//! (`NotFoundError`, `IsADirectoryError`, ...) to construct — see
//! [`throw_resolution`]/[`throw_io`]. The tag is a prefix added on top of a
//! normal, fully-detailed message (operation, virtual path, and the
//! underlying OS error text), never a replacement for it.

use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use crate::bindings::bind;
use rquickjs::{Ctx, Object, Result, TypedArray};

/// Why [`resolve_userland_path`]/[`resolve_userland_path_for_write`] failed
/// to resolve a requested path, carrying enough to build both a tag and a
/// rich message — see [`throw_resolution`].
pub enum PathResolutionError {
    /// `requested` didn't start with `/`. In practice unreachable from
    /// `ely:filesystem`'s exported functions, which all call
    /// `assertAbsolute` before ever reaching a native binding — kept only
    /// as defense in depth for a binding called some other way.
    NotAbsolute(String),
    /// A component of `requested` doesn't exist, or is a symlink (symlinks
    /// are invisible to the VM, so this is indistinguishable from not
    /// existing).
    NotFound(String),
    /// A component of `requested` isn't a normal path segment (e.g. a
    /// Windows-style drive prefix) — not reachable from a POSIX-style
    /// virtual path, kept only for exhaustiveness.
    Invalid(String),
}

impl PathResolutionError {
    fn tag(&self) -> &'static str {
        match self {
            PathResolutionError::NotAbsolute(_) => "NOT_FOUND",
            PathResolutionError::NotFound(_) => "NOT_FOUND",
            PathResolutionError::Invalid(_) => "IO_ERROR",
        }
    }

    fn message(&self) -> &str {
        match self {
            PathResolutionError::NotAbsolute(message)
            | PathResolutionError::NotFound(message)
            | PathResolutionError::Invalid(message) => message,
        }
    }
}

impl std::fmt::Display for PathResolutionError {
    /// The rich message without the machine-readable tag — for callers
    /// outside `ely:filesystem` (e.g. `ely:image`, program loading) that
    /// surface a plain error rather than parsing tags.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

/// Throws `err` as a tagged JS exception — see the module doc comment.
fn throw_resolution(ctx: &Ctx<'_>, err: PathResolutionError) -> rquickjs::Error {
    rquickjs::Exception::throw_message(ctx, &format!("{}: {}", err.tag(), err.message()))
}

/// Tags a `std::io::Error` from `operation` on `path` and throws it as a
/// tagged JS exception, keeping the full `path`/`operation`/OS-error detail
/// in the message — see the module doc comment.
fn throw_io(ctx: &Ctx<'_>, operation: &str, path: &str, err: std::io::Error) -> rquickjs::Error {
    let tag = match err.kind() {
        ErrorKind::NotFound => "NOT_FOUND",
        ErrorKind::IsADirectory => "IS_A_DIRECTORY",
        ErrorKind::NotADirectory => "NOT_A_DIRECTORY",
        _ => "IO_ERROR",
    };
    rquickjs::Exception::throw_message(ctx, &format!("{tag}: {operation} {path}: {err}"))
}

/// Like [`throw_io`], but for `std::fs::read_to_string` specifically: its
/// `ErrorKind::InvalidData` means the file's bytes aren't valid UTF-8,
/// which every other caller's `ErrorKind` set doesn't need to distinguish.
fn throw_read_to_string_io(ctx: &Ctx<'_>, path: &str, err: std::io::Error) -> rquickjs::Error {
    if err.kind() == ErrorKind::InvalidData {
        return rquickjs::Exception::throw_message(
            ctx,
            &format!("INVALID_UTF8: read {path}: {err}"),
        );
    }
    throw_io(ctx, "read", path, err)
}

/// Resolves `requested` — an absolute, virtual, userland-rooted path — to
/// its real path, requiring every component (including the leaf) to already
/// exist and none of them to be a symlink. `canonical_userland_root` must
/// already be canonicalized (as [`crate::runtime::ElysiumRuntime::new`]
/// does once for the whole VM); by construction the walk never produces a
/// path outside it, since `..` is clamped at the root exactly like
/// `ely:filesystem`'s own `normalize()`.
pub fn resolve_userland_path(
    canonical_userland_root: &Path,
    requested: &str,
) -> std::result::Result<PathBuf, PathResolutionError> {
    walk_userland_path(canonical_userland_root, requested, false)
}

/// Like [`resolve_userland_path`], but tolerates `requested`'s leaf (and any
/// number of trailing components) not existing yet — needed by
/// `createDirectory` (which may need to create several missing levels) and
/// by a `writeFile` targeting a not-yet-existing file.
pub fn resolve_userland_path_for_write(
    canonical_userland_root: &Path,
    requested: &str,
) -> std::result::Result<PathBuf, PathResolutionError> {
    walk_userland_path(canonical_userland_root, requested, true)
}

/// Shared walk behind [`resolve_userland_path`] and
/// [`resolve_userland_path_for_write`]. Advances component by component from
/// `canonical_userland_root`, checking `symlink_metadata` at every step and
/// treating any symlink component as though it didn't exist, and clamps
/// `..` at the root so the result can never escape it.
///
/// When a component doesn't exist: if `allow_missing_tail` is false the
/// whole path is rejected as not found; if it is true, that component and
/// everything after it is appended lexically (still clamping `..`) without
/// touching the filesystem again — a component that doesn't exist yet can't
/// itself be a symlink, so no further check is needed past that point.
fn walk_userland_path(
    canonical_userland_root: &Path,
    requested: &str,
    allow_missing_tail: bool,
) -> std::result::Result<PathBuf, PathResolutionError> {
    let Some(relative) = requested.strip_prefix('/') else {
        return Err(PathResolutionError::NotAbsolute(format!(
            "{requested} is not an absolute path"
        )));
    };

    let not_found =
        || PathResolutionError::NotFound(format!("{requested}: no such file or directory"));
    let invalid = || PathResolutionError::Invalid(format!("{requested} is not a valid path"));

    let mut current = canonical_userland_root.to_path_buf();
    let mut components = Path::new(relative).components();

    while let Some(component) = components.next() {
        match component {
            Component::Normal(part) => {
                let candidate = current.join(part);
                match std::fs::symlink_metadata(&candidate) {
                    Ok(metadata) => {
                        if metadata.file_type().is_symlink() {
                            return Err(not_found());
                        }
                        current = candidate;
                    }
                    Err(_) if allow_missing_tail => {
                        current.push(part);
                        for remaining in components {
                            match remaining {
                                Component::Normal(part) => current.push(part),
                                Component::ParentDir => {
                                    if current != canonical_userland_root {
                                        current.pop();
                                    }
                                }
                                Component::CurDir | Component::RootDir => {}
                                _ => return Err(invalid()),
                            }
                        }
                        return Ok(current);
                    }
                    Err(_) => return Err(not_found()),
                }
            }
            Component::ParentDir => {
                if current != canonical_userland_root {
                    current.pop();
                }
            }
            Component::CurDir | Component::RootDir => {}
            _ => return Err(invalid()),
        }
    }

    Ok(current)
}

/// Expresses `resolved` — a real, absolute path known to be inside
/// `canonical_userland_root` — as a virtual, userland-rooted path (e.g.
/// `/programs/init/index.ts`), the form `EntryStat.path` is reported in.
fn virtual_path(canonical_userland_root: &Path, resolved: &Path) -> String {
    let relative = resolved
        .strip_prefix(canonical_userland_root)
        .unwrap_or(resolved);
    format!("/{}", relative.to_string_lossy())
}

/// Reads `resolved`'s bytes, optionally restricted to the range starting at
/// `offset` and spanning at most `length` bytes (`length < 0` means "to the
/// end of the file"). Used by `__fs_read_file`.
fn read_range(resolved: &Path, offset: i64, length: i64) -> std::io::Result<Vec<u8>> {
    let bytes = std::fs::read(resolved)?;
    let offset = offset.max(0) as usize;
    if offset >= bytes.len() {
        return Ok(Vec::new());
    }
    let end = if length < 0 {
        bytes.len()
    } else {
        bytes.len().min(offset.saturating_add(length as usize))
    };
    Ok(bytes[offset..end].to_vec())
}

/// Writes `data` into `resolved` at `offset` (bytes before it are left
/// untouched). When `truncate` is set, the file is truncated to exactly
/// `data`'s length first — the "no range given, replace the whole file"
/// call `writeFile` makes when its caller didn't pass a `ByteRange`. Used by
/// `__fs_write_file`.
fn write_range(resolved: &Path, data: &[u8], offset: i64, truncate: bool) -> std::io::Result<()> {
    use std::io::{Seek, SeekFrom, Write};

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(truncate)
        .open(resolved)?;
    file.seek(SeekFrom::Start(offset.max(0) as u64))?;
    file.write_all(data)
}

/// Resolves `path` for reading, throwing the tagged JS exception on failure —
/// the opening move of every binding below that requires its target to exist.
fn resolve_for_read(ctx: &Ctx<'_>, root: &Path, path: &str) -> Result<PathBuf> {
    resolve_userland_path(root, path).map_err(|err| throw_resolution(ctx, err))
}

/// Like [`resolve_for_read`], but for the bindings that may be naming
/// something not created yet.
fn resolve_for_write(ctx: &Ctx<'_>, root: &Path, path: &str) -> Result<PathBuf> {
    resolve_userland_path_for_write(root, path).map_err(|err| throw_resolution(ctx, err))
}

/// Binds the *hidden* globals `ely:filesystem`'s embedded module wraps
/// (`__fs_read_file`, `__fs_write_file`, ...) — never called by a program
/// directly, only through `ely:filesystem`'s exported functions.
/// `userland_root` is already canonicalized by the caller
/// ([`crate::runtime::ElysiumRuntime::new`]).
pub fn bootstrap_filesystem_bindings<'js>(ctx: &Ctx<'js>, userland_root: PathBuf) -> Result<()> {
    {
        let userland_root = userland_root.clone();
        bind(
            ctx,
            "__fs_read_file",
            move |ctx: Ctx<'js>,
                  path: String,
                  offset: i64,
                  length: i64|
                  -> Result<TypedArray<'js, u8>> {
                let resolved = resolve_for_read(&ctx, &userland_root, &path)?;
                let bytes = read_range(&resolved, offset, length)
                    .map_err(|err| throw_io(&ctx, "read", &path, err))?;
                TypedArray::new_copy(ctx.clone(), bytes)
            },
        )?;
    }

    {
        let userland_root = userland_root.clone();
        bind(
            ctx,
            "__fs_write_file",
            move |ctx: Ctx<'_>,
                  path: String,
                  data: TypedArray<'_, u8>,
                  offset: i64,
                  truncate: bool|
                  -> Result<()> {
                let resolved = resolve_for_write(&ctx, &userland_root, &path)?;
                let bytes = data.as_bytes().ok_or_else(|| {
                    rquickjs::Exception::throw_message(
                        &ctx,
                        &format!("IO_ERROR: write {path}: data is not backed by an ArrayBuffer"),
                    )
                })?;
                write_range(&resolved, bytes, offset, truncate)
                    .map_err(|err| throw_io(&ctx, "write", &path, err))
            },
        )?;
    }

    {
        let userland_root = userland_root.clone();
        bind(
            ctx,
            "__fs_read_text_file",
            move |ctx: Ctx<'_>, path: String| -> Result<String> {
                let resolved = resolve_for_read(&ctx, &userland_root, &path)?;
                std::fs::read_to_string(&resolved)
                    .map_err(|err| throw_read_to_string_io(&ctx, &path, err))
            },
        )?;
    }

    {
        let userland_root = userland_root.clone();
        bind(
            ctx,
            "__fs_write_text_file",
            move |ctx: Ctx<'_>, path: String, text: String| -> Result<()> {
                let resolved = resolve_for_write(&ctx, &userland_root, &path)?;
                std::fs::write(&resolved, text).map_err(|err| throw_io(&ctx, "write", &path, err))
            },
        )?;
    }

    {
        let userland_root = userland_root.clone();
        bind(
            ctx,
            "__fs_remove",
            move |ctx: Ctx<'_>, path: String| -> Result<()> {
                let resolved = resolve_for_read(&ctx, &userland_root, &path)?;
                let metadata = std::fs::metadata(&resolved)
                    .map_err(|err| throw_io(&ctx, "remove", &path, err))?;
                let result = if metadata.is_dir() {
                    std::fs::remove_dir_all(&resolved)
                } else {
                    std::fs::remove_file(&resolved)
                };
                result.map_err(|err| throw_io(&ctx, "remove", &path, err))
            },
        )?;
    }

    {
        let userland_root = userland_root.clone();
        bind(
            ctx,
            "__fs_create_directory",
            move |ctx: Ctx<'_>, path: String| -> Result<()> {
                let resolved = resolve_for_write(&ctx, &userland_root, &path)?;
                std::fs::create_dir_all(&resolved)
                    .map_err(|err| throw_io(&ctx, "create directory", &path, err))
            },
        )?;
    }

    {
        let userland_root = userland_root.clone();
        bind(
            ctx,
            "__fs_list_directory",
            move |ctx: Ctx<'js>, path: String| -> Result<Vec<Object<'js>>> {
                let resolved = resolve_for_read(&ctx, &userland_root, &path)?;
                let entries = std::fs::read_dir(&resolved)
                    .map_err(|err| throw_io(&ctx, "list directory", &path, err))?;

                let mut result = Vec::new();
                for entry in entries {
                    let entry =
                        entry.map_err(|err| throw_io(&ctx, "list directory", &path, err))?;
                    let file_type = entry
                        .file_type()
                        .map_err(|err| throw_io(&ctx, "list directory", &path, err))?;
                    // Only files and directories are userland-visible —
                    // symlinks are invisible (see the module doc
                    // comment) and anything else (sockets, devices,
                    // ...) is out of scope for a files-and-directories
                    // filesystem API.
                    if !file_type.is_file() && !file_type.is_dir() {
                        continue;
                    }

                    let entry_path = virtual_path(&userland_root, &entry.path());
                    let object = Object::new(ctx.clone())?;
                    if file_type.is_dir() {
                        object.set("kind", "Directory")?;
                    } else {
                        let metadata = entry
                            .metadata()
                            .map_err(|err| throw_io(&ctx, "list directory", &path, err))?;
                        object.set("kind", "File")?;
                        object.set("size", metadata.len() as f64)?;
                    }
                    object.set("path", entry_path)?;
                    result.push(object);
                }
                Ok(result)
            },
        )?;
    }

    bind(
        ctx,
        "__fs_stat",
        move |ctx: Ctx<'js>, path: String| -> Result<Object<'js>> {
            let resolved = resolve_for_read(&ctx, &userland_root, &path)?;
            let metadata =
                std::fs::metadata(&resolved).map_err(|err| throw_io(&ctx, "stat", &path, err))?;

            let object = Object::new(ctx.clone())?;
            if metadata.is_dir() {
                object.set("kind", "Directory")?;
            } else if metadata.is_file() {
                object.set("kind", "File")?;
                object.set("size", metadata.len() as f64)?;
            } else {
                // Sockets, devices, etc. — outside the files-and-directories
                // scope `ely:filesystem` covers.
                return Err(rquickjs::Exception::throw_message(
                    &ctx,
                    &format!("IO_ERROR: stat {path}: not a file or directory"),
                ));
            }
            object.set("path", virtual_path(&userland_root, &resolved))?;
            Ok(object)
        },
    )?;

    Ok(())
}
