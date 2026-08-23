pub mod jsx;
pub mod no_top_level_await;
pub mod type_stripping;

/// Lowers a TS(X) source file to plain JS: JSX literals become `h(...)`
/// calls, then TypeScript-only syntax is erased. `import`/`export` are left
/// untouched, so the result is still valid module source. Also rejects
/// `await` used outside any function body — see
/// [`no_top_level_await::check_no_top_level_await`] for why.
pub fn compile(source: &str) -> Result<String, String> {
    no_top_level_await::check_no_top_level_await(source).map_err(|errors| {
        errors
            .iter()
            .map(|err| format!("{err:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    let source = jsx::transform_jsx(source).map_err(|errors| {
        errors
            .iter()
            .map(|err| format!("{err:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    })?;

    type_stripping::strip_types(&source).map_err(|errors| {
        errors
            .iter()
            .map(|err| format!("{err:?}"))
            .collect::<Vec<_>>()
            .join("\n")
    })
}
