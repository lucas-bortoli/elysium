mod runtime;

use runtime::ElysiumRuntime;

pub mod transform;

fn main() {
    let source = transform::type_stripping::strip_types(r#"print('hi' satisfies string)"#)
        .unwrap_or_else(|errors| {
            for err in &errors {
                eprintln!("type-stripping error: {err:?}");
            }
            std::process::exit(1);
        });

    let rt = ElysiumRuntime::new().expect("failed to initialize Elysium runtime");
    if let Err(err) = rt.eval(&source) {
        eprintln!("uncaught exception: {err}");
        std::process::exit(1);
    }
}
