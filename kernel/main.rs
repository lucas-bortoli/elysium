mod runtime;

use runtime::{ElysiumRuntime, GuardedError};

pub mod transform;

fn main() {
    let path = "userland/programs/init/index.ts";
    let program =
        std::fs::read_to_string(path).unwrap_or_else(|err| panic!("failed to read {path}: {err}"));

    let rt = ElysiumRuntime::new().expect("failed to initialize Elysium runtime");
    match rt.eval_module(path, &program) {
        Ok(()) => {}
        Err(GuardedError::Timeout) => {
            eprintln!("program timed out");
            std::process::exit(1);
        }
        Err(GuardedError::Exception(err)) => {
            eprintln!("uncaught exception: {err}");
            std::process::exit(1);
        }
    }
}
