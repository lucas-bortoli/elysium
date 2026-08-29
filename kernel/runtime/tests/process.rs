//! The `ely:process` surface: process identity, message-handler registration,
//! exit signalling, idle detection, message delivery, and deadlock remapping.

use super::*;

#[test]
fn process_surface_reports_defaults_for_a_detached_runtime() {
    let runtime = eval(
        "import { currentProcessId, currentArguments } from 'ely:process'; \
         globalThis.id = currentProcessId(); \
         globalThis.hasArgs = currentArguments() !== undefined;",
    );
    assert_eq!(global::<f64>(&runtime, "id"), 0.0);
    assert!(!global::<bool>(&runtime, "hasArgs"));
    assert!(!runtime.has_message_handler());
    assert!(!runtime.exit_requested());
}

#[test]
fn on_message_registration_is_visible_to_the_host() {
    let runtime =
        eval("import { addMessageHandler } from 'ely:process'; addMessageHandler(() => {});");
    assert!(runtime.has_message_handler());
}

#[test]
fn exit_binding_sets_the_flag_the_manager_reads() {
    let runtime = eval("import { exit } from 'ely:process'; exit();");
    assert!(runtime.exit_requested());
}

#[test]
fn has_no_pending_work_reflects_a_live_timer() {
    let runtime = eval("globalThis.id = setInterval(() => {}, 1000);");
    assert!(!runtime.has_no_pending_work());
    let id: f64 = global(&runtime, "id");
    runtime
        .context
        .with(|ctx| ctx.eval::<(), _>(format!("clearInterval({id});")))
        .unwrap();
    assert!(runtime.has_no_pending_work());
}

#[test]
fn deliver_message_invokes_the_registered_handler() {
    let runtime = eval(
        "import { addMessageHandler } from 'ely:process'; \
         globalThis.seen = null; \
         addMessageHandler((env) => { globalThis.seen = env.kind + ':' + env.data; });",
    );
    runtime
        .deliver_message(r#"{"kind":"greet","from":2,"to":0,"data":"hi"}"#)
        .unwrap();
    assert_eq!(global::<String>(&runtime, "seen"), "greet:hi");
}

#[test]
fn deadlock_exception_is_remapped_to_a_clear_message() {
    let remapped = remap_deadlock_error(GuardedError::Exception(
        "Error blocking on a promise resulted in a dead lock".to_string(),
    ));
    match remapped {
        GuardedError::Exception(message) => {
            assert!(message.contains("addPostInitHandler"));
            assert!(message.contains("ely:lifecycle"));
        }
        GuardedError::Timeout => panic!("expected an Exception"),
    }
}

#[test]
fn unrelated_exceptions_are_not_remapped() {
    let remapped = remap_deadlock_error(GuardedError::Exception("boom".to_string()));
    match remapped {
        GuardedError::Exception(message) => assert_eq!(message, "boom"),
        GuardedError::Timeout => panic!("expected an Exception"),
    }
}

#[test]
fn is_live_tells_a_spawned_process_from_one_that_never_existed() {
    // `spawn` marks its new id live straight away, before the process is
    // installed, so a program can use the id the moment it has it. Whether
    // a *terminated* process reports dead is the ProcessManager's half of
    // this — it owns the forget — and isn't reachable from a detached
    // runtime.
    let runtime = eval(
        "import { spawn, isLive } from 'ely:process'; \
         const child = spawn('/programs/whatever/index.ts', undefined); \
         globalThis.spawnedIsLive = isLive(child); \
         globalThis.strangerIsLive = isLive(999999);",
    );
    assert!(global::<bool>(&runtime, "spawnedIsLive"));
    assert!(!global::<bool>(&runtime, "strangerIsLive"));
}
