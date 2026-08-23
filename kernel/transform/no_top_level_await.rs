//! Rejects `await` used outside any function body. Elysium evaluates a
//! program's top-level module code as a single bounded, synchronous call
//! (see `ElysiumRuntime::eval_module`) that returns before the frame loop —
//! and therefore `setTimeout`/tickers/draw handlers — exists. A top-level
//! `await` on anything that isn't already resolved deadlocks the engine
//! instead of ever settling, so it's rejected here at compile time rather
//! than left to fail unpredictably at runtime.

use std::ops::Range;

use swc_common::sync::Lrc;
use swc_common::{BytePos, FileName, SourceMap, Span};
use swc_ecma_ast::*;
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax, lexer::Lexer};
use swc_ecma_visit::{Visit, VisitWith};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TopLevelAwaitError {
    /// An `await` expression reached outside any function body.
    TopLevelAwait(Range<usize>),
    /// The source failed to parse as TypeScript.
    ParseError(String),
}

/// Checks `source` for `await` used outside any function body. Never
/// modifies the source — `source` is returned unchanged on success, so this
/// can be slotted into the compile pipeline as a pure validation pass.
pub fn check_no_top_level_await(source: &str) -> Result<(), Vec<TopLevelAwaitError>> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(Lrc::new(FileName::Anon), source.to_string());
    let file_start = fm.start_pos;

    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: true,
            decorators: true,
            ..Default::default()
        }),
        Default::default(),
        StringInput::from(&*fm),
        None,
    );
    let mut parser = Parser::new_from(lexer);
    let module = parser
        .parse_module()
        .map_err(|e| vec![TopLevelAwaitError::ParseError(format!("{:?}", e.kind()))])?;

    let mut checker = Checker {
        file_start,
        function_depth: 0,
        errors: Vec::new(),
    };
    module.visit_with(&mut checker);

    if checker.errors.is_empty() {
        Ok(())
    } else {
        Err(checker.errors)
    }
}

struct Checker {
    file_start: BytePos,
    /// Depth of function bodies (regular functions, methods, getters/
    /// setters, constructors, arrow functions) the visitor is currently
    /// inside. An `AwaitExpr` seen at depth `0` is a top-level await.
    function_depth: u32,
    errors: Vec<TopLevelAwaitError>,
}

impl Checker {
    fn range(&self, span: Span) -> Range<usize> {
        let lo = (span.lo.0 - self.file_start.0) as usize;
        let hi = (span.hi.0 - self.file_start.0) as usize;
        lo..hi
    }
}

impl Visit for Checker {
    fn visit_function(&mut self, n: &Function) {
        self.function_depth += 1;
        n.visit_children_with(self);
        self.function_depth -= 1;
    }

    fn visit_constructor(&mut self, n: &Constructor) {
        self.function_depth += 1;
        n.visit_children_with(self);
        self.function_depth -= 1;
    }

    fn visit_arrow_expr(&mut self, n: &ArrowExpr) {
        self.function_depth += 1;
        n.visit_children_with(self);
        self.function_depth -= 1;
    }

    fn visit_await_expr(&mut self, n: &AwaitExpr) {
        if self.function_depth == 0 {
            self.errors
                .push(TopLevelAwaitError::TopLevelAwait(self.range(n.span)));
        }
        n.visit_children_with(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_await_is_rejected() {
        let input = "await new Promise((r) => setTimeout(r, 100));";
        let err = check_no_top_level_await(input).unwrap_err();
        assert!(matches!(err[0], TopLevelAwaitError::TopLevelAwait(_)));
    }

    #[test]
    fn await_inside_top_level_if_is_rejected() {
        let input = "if (true) { await Promise.resolve(); }";
        let err = check_no_top_level_await(input).unwrap_err();
        assert!(matches!(err[0], TopLevelAwaitError::TopLevelAwait(_)));
    }

    #[test]
    fn await_inside_async_function_is_allowed() {
        let input = "async function f() { await Promise.resolve(); }";
        assert!(check_no_top_level_await(input).is_ok());
    }

    #[test]
    fn await_inside_async_arrow_is_allowed() {
        let input = "const f = async () => { await Promise.resolve(); };";
        assert!(check_no_top_level_await(input).is_ok());
    }

    #[test]
    fn await_inside_async_method_is_allowed() {
        let input = "class Foo { async bar() { await Promise.resolve(); } }";
        assert!(check_no_top_level_await(input).is_ok());
    }

    #[test]
    fn no_await_is_allowed() {
        let input = "const x = 1;";
        assert!(check_no_top_level_await(input).is_ok());
    }
}
