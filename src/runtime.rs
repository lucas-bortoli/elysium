use rquickjs::{Context, Function, Runtime as JsRuntime};

pub struct ElysiumRuntime {
    _js_runtime: JsRuntime,
    context: Context,
}

impl ElysiumRuntime {
    pub fn new() -> Result<Self, rquickjs::Error> {
        let js_runtime = JsRuntime::new()?;
        let context = Context::full(&js_runtime)?;

        context.with(|ctx| -> Result<(), rquickjs::Error> {
            let global = ctx.globals();
            global.set(
                "print",
                Function::new(ctx.clone(), |msg: String| {
                    println!("{msg}");
                })?,
            )?;
            Ok(())
        })?;

        Ok(Self {
            _js_runtime: js_runtime,
            context,
        })
    }

    pub fn eval(&self, source: &str) -> Result<(), String> {
        self.context.with(|ctx| -> Result<(), String> {
            ctx.eval::<(), _>(source).map_err(|err| {
                if let rquickjs::Error::Exception = err {
                    format!("{}", ctx.catch().as_exception().unwrap())
                } else {
                    err.to_string()
                }
            })
        })
    }
}
