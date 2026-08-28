//! Module resolution: relative imports (resolved and rejected) and
//! `import.meta` virtual paths.

use super::*;

#[test]
fn relative_import_resolves_and_evaluates() {
    let entry = test_userland_root().join("entry.ts");
    let (runtime, _input) = eval_named_with_input(
        entry.to_str().unwrap(),
        "import { value } from './relative_import_target.ts'; \
         globalThis.value = value;",
    );
    assert_eq!(global::<f64>(&runtime, "value"), 42.0);
}

#[test]
fn relative_import_escaping_userland_root_fails_to_resolve() {
    let entry = test_userland_root().join("entry.ts");
    let (runtime, _input) = build_runtime(test_userland_root());
    let result = runtime.eval_module(entry.to_str().unwrap(), "import '../../../../etc/passwd';");
    assert!(result.is_err());
}

#[test]
fn import_meta_reports_virtual_userland_paths() {
    let module = test_userland_root().join("meta_module.ts");
    let (runtime, _input) = eval_named_with_input(
        module.to_str().unwrap(),
        "globalThis.directoryName = import.meta.directoryName; \
         globalThis.fileName = import.meta.fileName;",
    );
    assert_eq!(global::<String>(&runtime, "directoryName"), "/");
    assert_eq!(global::<String>(&runtime, "fileName"), "/meta_module.ts");
}
