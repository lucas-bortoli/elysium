//! The `ely:container` surface: `Option` construction, querying, and
//! unwrapping.

use super::*;

#[test]
fn container_has_value_and_is_option_narrow_correctly() {
    let runtime = eval(
        "import { hasValue, isOption, none, some } from 'ely:container'; \
         globalThis.presentHasValue = hasValue(some(1)); \
         globalThis.absentHasValue = hasValue(none()); \
         globalThis.isOptionAlways = isOption('anything');",
    );
    assert!(global::<bool>(&runtime, "presentHasValue"));
    assert!(!global::<bool>(&runtime, "absentHasValue"));
    assert!(global::<bool>(&runtime, "isOptionAlways"));
}

#[test]
fn container_get_or_else_returns_fallback_only_when_absent() {
    let runtime = eval(
        "import { getOrElse, none, some } from 'ely:container'; \
         globalThis.present = getOrElse(some(1), 2); \
         globalThis.absent = getOrElse(none(), 2);",
    );
    assert_eq!(global::<f64>(&runtime, "present"), 1.0);
    assert_eq!(global::<f64>(&runtime, "absent"), 2.0);
}

#[test]
fn container_map_transforms_present_and_passes_through_absent() {
    let runtime = eval(
        "import { map, none, some } from 'ely:container'; \
         globalThis.present = map(some(2), (x) => x * 10); \
         globalThis.absentIsUndefined = map(none(), (x) => x * 10) === undefined;",
    );
    assert_eq!(global::<f64>(&runtime, "present"), 20.0);
    assert!(global::<bool>(&runtime, "absentIsUndefined"));
}

#[test]
fn container_unwrap_throws_option_unwrap_error_on_empty_option() {
    let runtime = eval(
        "import { unwrap, none, some, OptionUnwrapError } from 'ely:container'; \
         globalThis.unwrapped = unwrap(some(42)); \
         globalThis.threw = false; \
         globalThis.correctType = false; \
         try { \
             unwrap(none()); \
         } catch (err) { \
             globalThis.threw = true; \
             globalThis.correctType = err instanceof OptionUnwrapError; \
         }",
    );
    assert_eq!(global::<f64>(&runtime, "unwrapped"), 42.0);
    assert!(global::<bool>(&runtime, "threw"));
    assert!(global::<bool>(&runtime, "correctType"));
}
