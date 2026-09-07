pub mod jsx;
pub mod no_top_level_await;
pub mod type_stripping;

/// Lowers a TS(X) source file to plain JS: JSX literals become `h(...)`
/// calls, then TypeScript-only syntax is erased. `import`/`export` are left
/// untouched, so the result is still valid module source. Also rejects
/// `await` used outside any function body — see
/// [`no_top_level_await::check_no_top_level_await`] for why.
pub fn compile(source: &str) -> Result<String, String> {
    no_top_level_await::check_no_top_level_await(source).map_err(join_errors)?;
    let source = jsx::transform_jsx(source).map_err(join_errors)?;
    type_stripping::strip_types(&source).map_err(join_errors)
}

/// A `.js` module (a vendored, pre-built bundle — see `userland/lib/`) is
/// already plain JavaScript: it needs neither the JSX rewrite nor type
/// stripping, and running either over arbitrary third-party output only
/// risks mangling code that was fine. The one pass that still applies is
/// the top-level-`await` rejection, since that guards how the VM evaluates
/// a module rather than the language it was written in. On success the
/// source is handed back untouched.
pub fn check_prebuilt_js(source: &str) -> Result<String, String> {
    no_top_level_await::check_no_top_level_await(source).map_err(join_errors)?;
    Ok(source.to_owned())
}

/// Every stage reports a batch of diagnostics; a caller only ever shows the
/// text, so collapse a batch into one message, a diagnostic per line.
fn join_errors<E: std::fmt::Debug>(errors: Vec<E>) -> String {
    errors
        .iter()
        .map(|err| format!("{err:?}"))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_prebuilt_js_hands_the_source_back_byte_for_byte() {
        let source = "export const y = 1 < 2;\nfunction f() { return `${y}`; }\n";
        assert_eq!(check_prebuilt_js(source).unwrap(), source);
    }

    #[test]
    fn check_prebuilt_js_runs_none_of_the_passes_compile_would() {
        // A bare `enum` is rejected by `compile` but is not this pass's
        // concern — a pre-built bundle is trusted to be plain JS already.
        assert!(compile("enum Dir { Up, Down }").is_err());
        assert!(check_prebuilt_js("enum Dir { Up, Down }").is_ok());
    }

    #[test]
    fn check_prebuilt_js_still_rejects_top_level_await() {
        assert!(check_prebuilt_js("await fetch();").is_err());
    }
}
