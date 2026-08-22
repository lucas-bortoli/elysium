mod runtime;

use runtime::ElysiumRuntime;

pub mod transform;

fn main() {
    let path = "programs/main.ts";
    let program =
        std::fs::read_to_string(path).unwrap_or_else(|err| panic!("failed to read {path}: {err}"));

    let rt = ElysiumRuntime::new().expect("failed to initialize Elysium runtime");
    if let Err(err) = rt.eval_module(path, &program) {
        eprintln!("uncaught exception: {err}");
        std::process::exit(1);
    }
}
