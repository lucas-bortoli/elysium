//! The one way a device installs a hidden global into a VM.
//!
//! Every `ely:` module is a thin TypeScript wrapper over a set of
//! `__device_operation` globals the kernel installs when a VM is built. Those
//! globals are the actual device boundary: a program never names one, it calls
//! the wrapper, which calls the global, which pushes onto whatever shared
//! buffer or queue the kernel drains between turns.
//!
//! Registering one is always the same three steps — build a native function
//! over some captured kernel state, and set it on the global object under its
//! name — so [`bind`] is the whole vocabulary the `bootstrap_*_bindings`
//! functions need. Anything a binding closure captures is cloned into it at
//! the call site, which is what keeps each registration to a single
//! expression.

use rquickjs::function::IntoJsFunc;
use rquickjs::{Ctx, Function, Result};

/// Installs `f` as the global `name` in `ctx`'s VM.
pub fn bind<'js, F, P>(ctx: &Ctx<'js>, name: &str, f: F) -> Result<()>
where
    F: IntoJsFunc<'js, P> + 'js,
{
    ctx.globals().set(name, Function::new(ctx.clone(), f)?)
}
