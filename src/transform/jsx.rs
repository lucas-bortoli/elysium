//! Preact-like JSX transform: rewrites `<Tag prop={x}>child</Tag>` expressions
//! into `h(tag, props, ...children)` calls, leaving everything else in the
//! source byte-for-byte untouched. Unlike `type_stripping`, output length is
//! not preserved (JSX literals and their `h()` calls are rarely the same
//! length), so this produces ordinary source-to-source text splices instead
//! of same-length blanking.
//!
//! `Fragment` (used for `<>...</>`) and `h` itself are expected to be
//! provided by the host runtime/program, the same way Preact expects them
//! to be imported.

use std::ops::Range;

use swc_common::sync::Lrc;
use swc_common::{BytePos, FileName, SourceMap, Span, Spanned};
use swc_ecma_ast::*;
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax, lexer::Lexer};
use swc_ecma_visit::{Visit, VisitWith};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsxError {
    /// The source failed to parse as TSX.
    ParseError(String),
}

/// Transforms JSX syntax in `source` into `h(...)` calls. All non-JSX source
/// text is passed through unchanged.
pub fn transform_jsx(source: &str) -> Result<String, Vec<JsxError>> {
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
        .map_err(|e| vec![JsxError::ParseError(format!("{:?}", e.kind()))])?;

    let mut transformer = Transformer {
        source,
        file_start,
        splices: Vec::new(),
    };
    module.visit_with(&mut transformer);

    Ok(transformer.finish())
}

/// True if `s` can be used as a bare (unquoted) object property key.
fn is_valid_js_ident(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
}

struct Transformer<'a> {
    source: &'a str,
    file_start: BytePos,
    /// Recorded `(byte range in original source) -> (replacement text)`
    /// pairs, one per JSX element/fragment found, in the order they were
    /// finished visiting (children before their parents).
    splices: Vec<(Range<usize>, String)>,
}

impl<'a> Transformer<'a> {
    fn range(&self, span: Span) -> Range<usize> {
        let lo = (span.lo.0 - self.file_start.0) as usize;
        let hi = (span.hi.0 - self.file_start.0) as usize;
        lo..hi
    }

    /// Renders `range` of the original source, substituting in any
    /// already-recorded splices that fall within it (used to pull already
    /// jsx-transformed nested elements/fragments into a parent's text).
    fn render_range(&self, range: Range<usize>) -> String {
        let mut matches: Vec<&(Range<usize>, String)> = self
            .splices
            .iter()
            .filter(|(r, _)| r.start >= range.start && r.end <= range.end)
            .collect();
        matches.sort_by_key(|(r, _)| r.start);

        let mut out = String::new();
        let mut cursor = range.start;
        for (r, text) in matches {
            if r.start < cursor {
                // Contained within an already-applied outer splice; skip.
                continue;
            }
            out.push_str(&self.source[cursor..r.start]);
            out.push_str(text);
            cursor = r.end;
        }
        out.push_str(&self.source[cursor..range.end]);
        out
    }

    fn render_expr(&self, expr: &Expr) -> String {
        self.render_range(self.range(expr.span()))
    }

    fn jsx_element_name_text(&self, name: &JSXElementName) -> (String, bool) {
        match name {
            JSXElementName::Ident(ident) => {
                let raw = ident.sym.as_ref();
                let is_component = raw
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_uppercase())
                    .unwrap_or(false);
                if is_component {
                    (raw.to_string(), true)
                } else {
                    (format!("{:?}", raw), false)
                }
            }
            JSXElementName::JSXMemberExpr(_) | JSXElementName::JSXNamespacedName(_) => {
                (self.render_range(self.range(name.span())), true)
            }
        }
    }

    fn jsx_attr_name_text(&self, name: &JSXAttrName) -> String {
        match name {
            JSXAttrName::Ident(ident) => {
                let raw = ident.sym.as_ref();
                if is_valid_js_ident(raw) {
                    raw.to_string()
                } else {
                    format!("{:?}", raw)
                }
            }
            JSXAttrName::JSXNamespacedName(ns) => {
                format!("{:?}", format!("{}:{}", ns.ns.sym, ns.name.sym))
            }
        }
    }

    fn render_attr_value(&self, value: &JSXAttrValue) -> String {
        match value {
            JSXAttrValue::Str(s) => format!("{:?}", s.value.to_string_lossy()),
            JSXAttrValue::JSXExprContainer(container) => match &container.expr {
                JSXExpr::JSXEmptyExpr(_) => "undefined".to_string(),
                JSXExpr::Expr(expr) => self.render_expr(expr),
            },
            JSXAttrValue::JSXElement(el) => self.render_range(self.range(el.span())),
            JSXAttrValue::JSXFragment(frag) => self.render_range(self.range(frag.span())),
        }
    }

    fn render_props(&self, attrs: &[JSXAttrOrSpread]) -> String {
        if attrs.is_empty() {
            return "null".to_string();
        }
        let mut parts = Vec::with_capacity(attrs.len());
        for attr in attrs {
            match attr {
                JSXAttrOrSpread::JSXAttr(attr) => {
                    let key = self.jsx_attr_name_text(&attr.name);
                    let value = match &attr.value {
                        Some(v) => self.render_attr_value(v),
                        None => "true".to_string(),
                    };
                    parts.push(format!("{}: {}", key, value));
                }
                JSXAttrOrSpread::SpreadElement(spread) => {
                    parts.push(format!("...{}", self.render_expr(&spread.expr)));
                }
            }
        }
        format!("{{{}}}", parts.join(", "))
    }

    /// Ports Babel's `cleanJSXElementLiteralChild`: collapses interior
    /// whitespace-only lines and trims leading/trailing blank runs, the same
    /// way JSX text children are normalized before becoming string literals.
    fn clean_jsx_text(value: &str) -> Option<String> {
        let lines: Vec<&str> = value.split(['\n', '\r']).collect();
        let last_non_empty_line = lines
            .iter()
            .rposition(|line| line.chars().any(|c| c != ' ' && c != '\t'))
            .unwrap_or(0);

        let mut out = String::new();
        for (i, line) in lines.iter().enumerate() {
            let is_first_line = i == 0;
            let is_last_line = i == lines.len() - 1;
            let is_last_non_empty_line = i == last_non_empty_line;

            let mut trimmed = line.replace('\t', " ");
            if !is_first_line {
                trimmed = trimmed.trim_start_matches(' ').to_string();
            }
            if !is_last_line {
                trimmed = trimmed.trim_end_matches(' ').to_string();
            }

            if !trimmed.is_empty() {
                if !is_last_non_empty_line {
                    trimmed.push(' ');
                }
                out.push_str(&trimmed);
            }
        }

        if out.is_empty() { None } else { Some(out) }
    }

    fn render_children(&self, children: &[JSXElementChild]) -> Vec<String> {
        let mut out = Vec::new();
        for child in children {
            match child {
                JSXElementChild::JSXText(text) => {
                    if let Some(cleaned) = Self::clean_jsx_text(&text.value.to_string_lossy()) {
                        out.push(format!("{:?}", cleaned));
                    }
                }
                JSXElementChild::JSXExprContainer(container) => match &container.expr {
                    JSXExpr::JSXEmptyExpr(_) => {}
                    JSXExpr::Expr(expr) => out.push(self.render_expr(expr)),
                },
                JSXElementChild::JSXElement(el) => {
                    out.push(self.render_range(self.range(el.span())))
                }
                JSXElementChild::JSXFragment(frag) => {
                    out.push(self.render_range(self.range(frag.span())))
                }
                JSXElementChild::JSXSpreadChild(spread) => {
                    out.push(format!("...{}", self.render_expr(&spread.expr)))
                }
            }
        }
        out
    }

    fn render_h_call(
        &self,
        tag: &str,
        attrs: &[JSXAttrOrSpread],
        children: &[JSXElementChild],
    ) -> String {
        let props = self.render_props(attrs);
        let children = self.render_children(children);
        let mut args = vec![tag.to_string(), props];
        args.extend(children);
        format!("h({})", args.join(", "))
    }
}

impl<'a> Visit for Transformer<'a> {
    fn visit_jsx_element(&mut self, n: &JSXElement) {
        n.visit_children_with(self);

        let (tag, _is_component) = self.jsx_element_name_text(&n.opening.name);
        let text = self.render_h_call(&tag, &n.opening.attrs, &n.children);
        self.splices.push((self.range(n.span), text));
    }

    fn visit_jsx_fragment(&mut self, n: &JSXFragment) {
        n.visit_children_with(self);

        let children = self.render_children(&n.children);
        let mut args = vec!["Fragment".to_string(), "null".to_string()];
        args.extend(children);
        let text = format!("h({})", args.join(", "));
        self.splices.push((self.range(n.span), text));
    }
}

impl<'a> Transformer<'a> {
    fn finish(self) -> String {
        let range = 0..self.source.len();
        self.render_range(range)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_element() {
        let out = transform_jsx("const el = <div>hello</div>;").unwrap();
        assert_eq!(out, "const el = h(\"div\", null, \"hello\");");
    }

    #[test]
    fn element_with_attrs() {
        let out = transform_jsx("const el = <div id=\"x\" hidden>hi</div>;").unwrap();
        assert_eq!(
            out,
            "const el = h(\"div\", {id: \"x\", hidden: true}, \"hi\");",
        );
    }

    #[test]
    fn component_reference() {
        let out = transform_jsx("const el = <Foo.Bar name={name} />;").unwrap();
        assert_eq!(out, "const el = h(Foo.Bar, {name: name});");
    }

    #[test]
    fn nested_elements() {
        let out = transform_jsx("const el = <div><span>a</span><span>b</span></div>;").unwrap();
        assert_eq!(
            out,
            "const el = h(\"div\", null, h(\"span\", null, \"a\"), h(\"span\", null, \"b\"));"
        );
    }

    #[test]
    fn expression_child() {
        let out = transform_jsx("const el = <div>{value}</div>;").unwrap();
        assert_eq!(out, "const el = h(\"div\", null, value);");
    }

    #[test]
    fn jsx_inside_ternary() {
        let out = transform_jsx("const el = cond ? <div>a</div> : <span>b</span>;").unwrap();
        assert_eq!(
            out,
            "const el = cond ? h(\"div\", null, \"a\") : h(\"span\", null, \"b\");"
        );
    }

    #[test]
    fn fragment_is_h_call() {
        let out = transform_jsx("const el = <>{a}{b}</>;").unwrap();
        assert_eq!(out, "const el = h(Fragment, null, a, b);");
    }

    #[test]
    fn spread_attrs_and_children() {
        let out = transform_jsx("const el = <div {...props}>{...items}</div>;").unwrap();
        assert_eq!(out, "const el = h(\"div\", {...props}, ...items);");
    }

    #[test]
    fn non_jsx_source_is_untouched() {
        let input = "function f(a: number): number { return a + 1; }\nconst el = <div/>;";
        let out = transform_jsx(input).unwrap();
        assert!(out.starts_with("function f(a: number): number { return a + 1; }\n"));
        assert!(out.contains("h(\"div\", null)"));
    }
}
