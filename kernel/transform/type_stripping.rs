//! Strips TypeScript-only syntax from a source string, replacing it with
//! whitespace of equal byte length so every remaining JS token keeps its
//! original position (no source map needed). Port of the algorithm used by
//! https://github.com/bloomberg/ts-blank-space.

use std::ops::Range;

use swc_common::sync::Lrc;
use swc_common::{BytePos, FileName, SourceMap, Span, Spanned};
use swc_ecma_ast::*;
use swc_ecma_parser::{Parser, StringInput, Syntax, TsSyntax, lexer::Lexer};
use swc_ecma_visit::{Visit, VisitWith};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripError {
    /// A non-`declare` `enum` has runtime semantics and cannot be erased.
    UnsupportedEnum(Range<usize>),
    /// A namespace/module whose body has runtime content cannot be erased.
    UnsupportedInstantiatedNamespace(Range<usize>),
    /// `import x = ...` / `export = ...` has runtime semantics.
    UnsupportedImportEquals(Range<usize>),
    /// Constructor parameter properties synthesize a runtime assignment.
    UnsupportedParameterProperty(Range<usize>),
    /// Legacy `<T>value` prefix casts are ambiguous with JSX and are never erased.
    UnsupportedLegacyCast(Range<usize>),
    /// The source failed to parse as TypeScript.
    ParseError(String),
}

/// Strips TypeScript type syntax from `source`, returning JS of the same
/// byte length (line/column positions of all remaining tokens are preserved).
pub fn strip_types(source: &str) -> Result<String, Vec<StripError>> {
    let cm: Lrc<SourceMap> = Default::default();
    let fm = cm.new_source_file(Lrc::new(FileName::Anon), source.to_string());
    let file_start = fm.start_pos;

    let lexer = Lexer::new(
        Syntax::Typescript(TsSyntax {
            tsx: false,
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
        .map_err(|e| vec![StripError::ParseError(format!("{:?}", e.kind()))])?;

    let mut stripper = Stripper {
        blank: Blank::new(source),
        file_start,
        errors: Vec::new(),
        semicolon_needed: false,
        parent_statement_end: None,
    };
    module.visit_with(&mut stripper);

    if stripper.errors.is_empty() {
        Ok(stripper.blank.finish())
    } else {
        Err(stripper.errors)
    }
}

/// A byte buffer initialized as a copy of the source, supporting in-place
/// blanking of ranges (preserving newlines) and single-byte substitutions.
struct Blank {
    bytes: Vec<u8>,
}

impl Blank {
    fn new(source: &str) -> Self {
        Self {
            bytes: source.as_bytes().to_vec(),
        }
    }

    fn blank_range(&mut self, r: Range<usize>) {
        for b in &mut self.bytes[r] {
            if *b != b'\n' && *b != b'\r' {
                *b = b' ';
            }
        }
    }

    fn replace_at(&mut self, pos: usize, byte: u8) {
        self.bytes[pos] = byte;
    }

    /// Blanks the first occurrence of `ch` within `[from, to)`, if any.
    /// Used for single-character TS markers (`?`, `!`) whose exact position
    /// isn't captured by a dedicated span in the AST, but which are known to
    /// be the only occurrence of `ch` in a narrow, parser-validated window.
    fn blank_first_char_in(&mut self, from: usize, to: usize, ch: u8) {
        if let Some(pos) = self.bytes[from..to].iter().position(|&b| b == ch) {
            self.bytes[from + pos] = b' ';
        }
    }

    fn finish(self) -> String {
        // Safe: only ASCII whitespace bytes were ever substituted in place.
        String::from_utf8(self.bytes).expect("blanking only replaces bytes with ASCII spaces")
    }
}

/// Keywords that only matter to TypeScript and never affect JS runtime
/// semantics. `static` is a real ES keyword and must never appear here.
const MODIFIER_KEYWORDS: &[&str] = &[
    "declare",
    "public",
    "private",
    "protected",
    "readonly",
    "override",
    "abstract",
];

/// Real JS/ES keywords that can appear in the same modifier position but
/// are never blanked. Only used to detect when one of these precedes a
/// blanked keyword, which suppresses the semicolon-guard heuristic (see
/// `blank_modifiers_in_range`).
const KEPT_MODIFIER_KEYWORDS: &[&str] = &["static", "async", "accessor"];

struct Stripper {
    blank: Blank,
    file_start: BytePos,
    errors: Vec<StripError>,
    /// True when the immediately preceding statement in the current
    /// statement list was kept as real JS but didn't end with `;` in the
    /// source. A statement that gets fully blanked after this is true must
    /// have its blank range start with `;` instead, to preserve the
    /// now-orphaned prior statement's terminator (ASI safety).
    semicolon_needed: bool,
    /// Byte end of the statement currently being visited, used to detect
    /// when an `as`/`satisfies` cast is the tail of its statement (so
    /// erasing it needs a `;` guard against ASI hazards on the next line).
    parent_statement_end: Option<usize>,
}

impl Stripper {
    fn range(&self, span: Span) -> Range<usize> {
        let lo = (span.lo.0 - self.file_start.0) as usize;
        let hi = (span.hi.0 - self.file_start.0) as usize;
        lo..hi
    }

    fn blank_span(&mut self, span: Span) {
        let r = self.range(span);
        self.blank.blank_range(r);
    }

    /// Blanks `span` and, if the next non-whitespace byte after it is a
    /// `,`, blanks that too (a lone erased specifier in a list must not
    /// leave a dangling comma behind).
    fn blank_and_optional_trailing_comma(&mut self, span: Span) {
        let r = self.range(span);
        let mut end = r.end;
        loop {
            match self.blank.bytes.get(end) {
                Some(b) if b.is_ascii_whitespace() => end += 1,
                Some(b'/') if self.blank.bytes.get(end + 1) == Some(&b'*') => {
                    end += 2;
                    while end < self.blank.bytes.len()
                        && !(self.blank.bytes[end] == b'*'
                            && self.blank.bytes.get(end + 1) == Some(&b'/'))
                    {
                        end += 1;
                    }
                    end = (end + 2).min(self.blank.bytes.len());
                }
                Some(b',') => {
                    end += 1;
                    break;
                }
                _ => break,
            }
        }
        self.blank.blank_range(r.start..end);
    }

    /// Blanks a whole statement/declaration, prefixing with `;` instead of
    /// a space when the preceding statement in this list needs one (see
    /// `semicolon_needed`).
    fn blank_statement(&mut self, span: Span) {
        let r = self.range(span);
        if self.semicolon_needed && !r.is_empty() {
            let start = r.start;
            self.blank.blank_range(r);
            self.blank.replace_at(start, b';');
            self.semicolon_needed = false;
        } else {
            self.blank.blank_range(r);
        }
    }

    /// True if every byte in `r` is whitespace or `;` — i.e. this source
    /// range holds no surviving runtime JS after stripping.
    fn range_is_blanked(&self, r: Range<usize>) -> bool {
        self.blank.bytes[r]
            .iter()
            .all(|b| b.is_ascii_whitespace() || *b == b';')
    }

    /// True if any byte in `[from, to)` is a newline. Mirrors upstream's
    /// `spansLines`, used to decide whether erasing a multi-line construct
    /// risks an ASI hazard once collapsed to whitespace in place.
    fn spans_lines(&self, from: usize, to: usize) -> bool {
        self.blank.bytes[from..to].contains(&b'\n')
    }

    fn find_byte_forward(&self, from: usize, byte: u8) -> Option<usize> {
        self.blank.bytes[from..]
            .iter()
            .position(|&b| b == byte)
            .map(|i| from + i)
    }

    fn find_bytes_forward(&self, from: usize, bytes: &[u8]) -> Option<usize> {
        self.blank.bytes[from..]
            .windows(bytes.len())
            .position(|w| w == bytes)
            .map(|i| from + i)
    }

    /// Scans forward from an opening `(` at `open_pos` for its matching
    /// `)`, tracking `()`/`[]`/`{}` nesting depth (strings/comments/regex
    /// aren't specially handled — a rare source of false matches, accepted
    /// as a known limitation of this byte-level scan).
    fn find_matching_close_paren(&self, open_pos: usize) -> usize {
        let mut depth = 0i32;
        for (i, &b) in self.blank.bytes[open_pos..].iter().enumerate() {
            match b {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return open_pos + i;
                    }
                }
                _ => {}
            }
        }
        self.blank.bytes.len().saturating_sub(1)
    }

    /// Ports upstream's `visitNodeArray` semicolon-tracking: visits each
    /// item in a statement list in order, threading `semicolon_needed`
    /// through so a fully-blanked item knows whether the previous kept
    /// statement needs a `;` preserved. `reset` matches upstream's
    /// `isFunctionBody` flag: statement lists that start a new function
    /// body reset the flag to false on entry (nested ASI hazards can't leak
    /// out of a function boundary); other lists inherit the outer value.
    /// Visits a single-statement body position (an `if`/`while`/`for`
    /// branch without braces). Unlike list items, this position always
    /// needs *some* token: if the statement erases to pure whitespace, the
    /// following code would otherwise be parsed as this body instead of a
    /// sibling statement, so a `;` placeholder is planted unconditionally.
    fn visit_single_stmt_body(&mut self, stmt: &Stmt) {
        let r = self.range(stmt.span());
        stmt.visit_with(self);
        if !r.is_empty() && self.range_is_blanked(r.clone()) {
            self.blank.replace_at(r.start, b';');
        }
    }

    fn visit_statement_list<T>(&mut self, items: &[T], reset: bool)
    where
        T: VisitWith<Self> + Spanned,
    {
        let saved = self.semicolon_needed;
        let saved_end = self.parent_statement_end;
        if reset {
            self.semicolon_needed = false;
        }
        for item in items {
            let r = self.range(item.span());
            self.parent_statement_end = Some(r.end);
            item.visit_with(self);
            if !self.range_is_blanked(r.clone()) {
                self.semicolon_needed = !self.ends_with_semicolon(r.end);
            }
        }
        self.semicolon_needed = saved;
        self.parent_statement_end = saved_end;
    }

    /// Blanks the `as T` / `satisfies T` suffix of a cast, from the end of
    /// `expr_span` to the end of `whole_span`. If this cast is the tail of
    /// its enclosing statement and the source has no explicit `;`, the
    /// blanked range is prefixed with `;` — otherwise erasing the cast
    /// could let the next line's `(`, `[`, or `` ` `` merge into this
    /// expression via ASI.
    fn blank_cast_suffix(&mut self, expr_span: Span, whole_span: Span) {
        let expr_end = self.range(expr_span).end;
        let whole_end = self.range(whole_span).end;
        let is_statement_tail = self.parent_statement_end == Some(whole_end);
        let followed_by_semi = self.blank.bytes.get(whole_end) == Some(&b';');
        if is_statement_tail && !followed_by_semi {
            self.blank.blank_range(expr_end..whole_end);
            self.blank.replace_at(expr_end, b';');
        } else {
            self.blank.blank_range(expr_end..whole_end);
        }
    }

    /// Some node spans (e.g. swc's `ClassProp`) stop before a trailing `;`
    /// that's really part of the statement. Check the span's own last byte
    /// first, then look for an immediately-following `;` (past only
    /// whitespace) as a fallback.
    fn ends_with_semicolon(&self, end: usize) -> bool {
        if self.blank.bytes.get(end.wrapping_sub(1)) == Some(&b';') {
            return true;
        }
        let mut i = end;
        while let Some(b) = self.blank.bytes.get(i) {
            if b.is_ascii_whitespace() {
                i += 1;
            } else {
                return *b == b';';
            }
        }
        false
    }

    /// Blanks any TS-only modifier keywords found textually within
    /// `[from, to)`. swc's simplified AST records modifiers as booleans
    /// without their own spans, so this narrow, parser-validated window
    /// (start of a class member, or after its decorators, up to its key)
    /// can only contain modifier keywords, whitespace, and comments.
    /// Blanks any TS-only modifier keywords found textually within
    /// `[from, to)`. When `add_semi` is true (upstream: the member has a
    /// computed `[...]` key, the position most prone to an ASI hazard once
    /// its modifiers are erased), the first blanked keyword is
    /// unconditionally prefixed with `;` as a defensive guard — this does
    /// not depend on whether the previous member actually lacked a `;`.
    fn blank_modifiers_in_range(&mut self, from: usize, to: usize, add_semi: bool) {
        if from >= to {
            return;
        }
        let source = std::str::from_utf8(&self.blank.bytes[from..to])
            .expect("source is valid utf8")
            .to_string();
        let mut first_blanked: Option<usize> = None;
        for kw in MODIFIER_KEYWORDS {
            let mut search_from = 0;
            while let Some(idx) = source[search_from..].find(kw) {
                let start = search_from + idx;
                let end = start + kw.len();
                let before_ok = start == 0
                    || !source.as_bytes()[start - 1].is_ascii_alphanumeric()
                        && source.as_bytes()[start - 1] != b'_';
                let after_ok = end == source.len()
                    || !source.as_bytes()[end].is_ascii_alphanumeric()
                        && source.as_bytes()[end] != b'_';
                if before_ok && after_ok {
                    self.blank.blank_range((from + start)..(from + end));
                    first_blanked = Some(first_blanked.unwrap_or(from + start).min(from + start));
                }
                search_from = end;
            }
        }
        // Upstream only adds the `;` when the *very first* modifier token
        // (blanked or not, e.g. `static`/`async`) is itself a blanked one —
        // if a kept keyword like `static` comes first, no `;` is added even
        // though a later `readonly`/`public`/etc. still gets erased.
        if add_semi {
            if let Some(pos) = first_blanked {
                let kept_before = KEPT_MODIFIER_KEYWORDS.iter().any(|kw| {
                    source[..pos - from]
                        .rfind(kw)
                        .map(|idx| {
                            let start = idx;
                            let end = idx + kw.len();
                            let before_ok = start == 0
                                || !source.as_bytes()[start - 1].is_ascii_alphanumeric()
                                    && source.as_bytes()[start - 1] != b'_';
                            let after_ok = !source.as_bytes()[end].is_ascii_alphanumeric()
                                && source.as_bytes()[end] != b'_';
                            before_ok && after_ok
                        })
                        .unwrap_or(false)
                });
                if !kept_before {
                    self.blank.replace_at(pos, b';');
                }
            }
        }
    }

    fn blank_type_params(&mut self, type_params: &Option<Box<TsTypeParamDecl>>) {
        if let Some(tp) = type_params {
            self.blank_span(tp.span);
        }
    }

    fn blank_type_args(&mut self, type_args: &Option<Box<TsTypeParamInstantiation>>) {
        if let Some(ta) = type_args {
            self.blank_span(ta.span);
        }
    }

    fn blank_return_type(&mut self, return_type: &Option<Box<TsTypeAnn>>) {
        if let Some(rt) = return_type {
            self.blank_span(rt.span);
        }
    }

    /// `this` parameters exist only for the type checker and are never part
    /// of the runtime parameter list. Also blanks a trailing comma, if any,
    /// so erasing `this` doesn't leave a dangling `(, a)`.
    fn blank_this_param(&mut self, this_param: &Option<Box<TsThisParam>>) {
        if let Some(tp) = this_param {
            let lo = self.range(tp.span).start;
            let mut hi = self.range(tp.span).end;
            let mut i = hi;
            while let Some(b) = self.blank.bytes.get(i) {
                if b.is_ascii_whitespace() {
                    i += 1;
                } else if *b == b',' {
                    hi = i + 1;
                    break;
                } else {
                    break;
                }
            }
            self.blank.blank_range(lo..hi);
        }
    }

    fn strip_function(&mut self, function: &Function) {
        if function.body.is_none() {
            // Overload signature: no runtime code, blank entirely.
            self.blank_statement(function.span);
            return;
        }
        self.blank_this_param(&function.this_param);
        self.blank_type_params(&function.type_params);
        self.blank_return_type(&function.return_type);
        for param in &function.params {
            self.blank_param_type_and_optional(&param.pat);
        }
        function.visit_with(self);
    }

    fn blank_param_type_and_optional(&mut self, pat: &Pat) {
        if let Pat::Ident(bi) = pat {
            if bi.id.optional {
                let pos = (bi.id.span.hi.0 - self.file_start.0) as usize;
                self.blank.replace_at(pos, b' ');
            }
            if let Some(ann) = &bi.type_ann {
                self.blank_span(ann.span);
            }
        }
    }

    /// Ports `valueNamespaceWorker` from upstream: a purely syntactic check
    /// for whether a namespace body has any runtime-visible content.
    fn namespace_body_has_value(item: &ModuleItem) -> bool {
        match item {
            ModuleItem::Stmt(Stmt::Decl(decl)) => Self::decl_has_value(decl),
            ModuleItem::ModuleDecl(ModuleDecl::ExportDecl(export_decl)) => {
                Self::decl_has_value(&export_decl.decl)
            }
            ModuleItem::ModuleDecl(ModuleDecl::TsImportEquals(import_eq)) => import_eq.is_export,
            _ => true,
        }
    }

    fn module_should_blank(n: &TsModuleDecl) -> bool {
        let is_ambient_string_module = matches!(n.id, TsModuleName::Str(_));
        let is_value_free = match &n.body {
            None => true,
            Some(TsNamespaceBody::TsModuleBlock(block)) => {
                !block.body.iter().any(Self::namespace_body_has_value)
            }
            Some(TsNamespaceBody::TsNamespaceDecl(decl)) => !Self::namespace_decl_has_value(decl),
        };
        n.declare || is_ambient_string_module || n.global || is_value_free
    }

    fn decl_has_value(decl: &Decl) -> bool {
        match decl {
            Decl::TsInterface(_) | Decl::TsTypeAlias(_) => false,
            Decl::TsModule(m) => {
                if !m.namespace {
                    return true;
                }
                match &m.body {
                    None => false,
                    Some(TsNamespaceBody::TsModuleBlock(block)) => {
                        block.body.iter().any(Self::namespace_body_has_value)
                    }
                    Some(TsNamespaceBody::TsNamespaceDecl(decl)) => {
                        Self::namespace_decl_has_value(decl)
                    }
                }
            }
            _ => true,
        }
    }

    fn namespace_decl_has_value(decl: &TsNamespaceDecl) -> bool {
        match decl.body.as_ref() {
            TsNamespaceBody::TsModuleBlock(block) => {
                block.body.iter().any(Self::namespace_body_has_value)
            }
            TsNamespaceBody::TsNamespaceDecl(inner) => Self::namespace_decl_has_value(inner),
        }
    }
}

impl Visit for Stripper {
    fn visit_module_items(&mut self, n: &[ModuleItem]) {
        self.visit_statement_list(n, false);
    }

    fn visit_if_stmt(&mut self, n: &IfStmt) {
        n.test.visit_with(self);
        self.visit_single_stmt_body(&n.cons);
        if let Some(alt) = &n.alt {
            self.visit_single_stmt_body(alt);
        }
    }

    fn visit_stmts(&mut self, n: &[Stmt]) {
        self.visit_statement_list(n, true);
    }

    fn visit_class_members(&mut self, n: &[ClassMember]) {
        self.visit_statement_list(n, false);
    }

    fn visit_ts_interface_decl(&mut self, n: &TsInterfaceDecl) {
        self.blank_statement(n.span);
    }

    fn visit_ts_type_alias_decl(&mut self, n: &TsTypeAliasDecl) {
        self.blank_statement(n.span);
    }

    fn visit_ts_type_ann(&mut self, n: &TsTypeAnn) {
        self.blank_span(n.span);
    }

    fn visit_ts_index_signature(&mut self, n: &TsIndexSignature) {
        self.blank_span(n.span);
    }

    fn visit_ts_as_expr(&mut self, n: &TsAsExpr) {
        n.expr.visit_with(self);
        self.blank_cast_suffix(n.expr.span(), n.span);
    }

    /// `expr as const` parses to its own node distinct from `TsAsExpr`
    /// (which only covers `expr as SomeType`), so it needs its own override
    /// — without one, the trailing ` as const` is left as unerased TS
    /// syntax a JS parser can't read.
    fn visit_ts_const_assertion(&mut self, n: &TsConstAssertion) {
        n.expr.visit_with(self);
        self.blank_cast_suffix(n.expr.span(), n.span);
    }

    fn visit_ts_satisfies_expr(&mut self, n: &TsSatisfiesExpr) {
        n.expr.visit_with(self);
        self.blank_cast_suffix(n.expr.span(), n.span);
    }

    fn visit_ts_non_null_expr(&mut self, n: &TsNonNullExpr) {
        n.expr.visit_with(self);
        let pos = (n.span.hi.0 - self.file_start.0) as usize - 1;
        self.blank.replace_at(pos, b' ');
    }

    fn visit_ts_type_assertion(&mut self, n: &TsTypeAssertion) {
        self.errors
            .push(StripError::UnsupportedLegacyCast(self.range(n.span)));
        n.expr.visit_with(self);
    }

    fn visit_ts_enum_decl(&mut self, n: &TsEnumDecl) {
        if n.declare {
            self.blank_statement(n.span);
        } else {
            self.errors
                .push(StripError::UnsupportedEnum(self.range(n.span)));
        }
    }

    fn visit_ts_module_decl(&mut self, n: &TsModuleDecl) {
        if Self::module_should_blank(n) {
            self.blank_statement(n.span);
        } else {
            self.errors
                .push(StripError::UnsupportedInstantiatedNamespace(
                    self.range(n.span),
                ));
        }
    }

    fn visit_ts_import_equals_decl(&mut self, n: &TsImportEqualsDecl) {
        self.errors
            .push(StripError::UnsupportedImportEquals(self.range(n.span)));
    }

    fn visit_ts_export_assignment(&mut self, n: &TsExportAssignment) {
        self.errors
            .push(StripError::UnsupportedImportEquals(self.range(n.span)));
    }

    fn visit_ts_param_prop(&mut self, n: &TsParamProp) {
        self.errors
            .push(StripError::UnsupportedParameterProperty(self.range(n.span)));
    }

    fn visit_import_decl(&mut self, n: &ImportDecl) {
        if n.type_only {
            self.blank_statement(n.span);
            return;
        }
        for spec in &n.specifiers {
            if let ImportSpecifier::Named(named) = spec {
                if named.is_type_only {
                    self.blank_and_optional_trailing_comma(named.span);
                }
            }
        }
    }

    fn visit_named_export(&mut self, n: &NamedExport) {
        if n.type_only {
            self.blank_statement(n.span);
            return;
        }
        for spec in &n.specifiers {
            if let ExportSpecifier::Named(named) = spec {
                if named.is_type_only {
                    self.blank_and_optional_trailing_comma(named.span);
                }
            }
        }
    }

    fn visit_export_all(&mut self, n: &ExportAll) {
        if n.type_only {
            self.blank_statement(n.span);
        }
    }

    fn visit_fn_decl(&mut self, n: &FnDecl) {
        self.strip_function(&n.function);
    }

    fn visit_fn_expr(&mut self, n: &FnExpr) {
        self.strip_function(&n.function);
    }

    fn visit_export_decl(&mut self, n: &ExportDecl) {
        // `export` sits outside the wrapped declaration's own span, so a
        // decl that's fully blanked must have the `export` keyword blanked
        // along with it here, rather than by the inner per-decl visitor.
        let should_blank_whole = match &n.decl {
            Decl::TsInterface(_) | Decl::TsTypeAlias(_) => true,
            Decl::TsEnum(e) => e.declare,
            Decl::TsModule(m) => Self::module_should_blank(m),
            Decl::Class(c) => c.declare,
            Decl::Fn(f) => f.declare,
            Decl::Var(v) => v.declare,
            _ => false,
        };
        if should_blank_whole {
            self.blank_statement(n.span);
        } else {
            n.decl.visit_with(self);
        }
    }

    fn visit_var_decl(&mut self, n: &VarDecl) {
        if n.declare {
            self.blank_statement(n.span);
            return;
        }
        n.visit_children_with(self);
    }

    fn visit_class_decl(&mut self, n: &ClassDecl) {
        if n.declare {
            let start = n
                .class
                .decorators
                .first()
                .map(|d| d.span.lo)
                .unwrap_or(n.class.span.lo);
            self.blank_statement(Span::new(start, n.class.span.hi));
            return;
        }
        if n.class.is_abstract {
            let scan_from = n
                .class
                .decorators
                .last()
                .map(|d| d.span.hi)
                .unwrap_or(n.class.span.lo);
            let from = self.range(Span::new(scan_from, scan_from)).start;
            let to = self
                .range(Span::new(n.ident.span.lo, n.ident.span.lo))
                .start;
            self.blank_modifiers_in_range(from, to, false);
        }
        n.visit_children_with(self);
    }

    fn visit_class(&mut self, n: &Class) {
        self.blank_type_params(&n.type_params);
        self.blank_type_args(&n.super_type_params);
        if let (Some(first), Some(last)) = (n.implements.first(), n.implements.last()) {
            // Blank the whole `implements A, B` clause, including the
            // `implements` keyword and separating commas (neither has its
            // own span in this AST): scan backward from the first type
            // argument for the keyword's start.
            let scan_from = n
                .super_type_params
                .as_ref()
                .map(|tp| tp.span.hi)
                .or_else(|| n.super_class.as_ref().map(|s| s.span().hi))
                .unwrap_or(n.span.lo);
            let scan_from = (scan_from.0 - self.file_start.0) as usize;
            let first_lo = (first.span.lo.0 - self.file_start.0) as usize;
            let kw_start = self.blank.bytes[scan_from..first_lo]
                .windows(10)
                .position(|w| w == b"implements")
                .map(|off| scan_from + off)
                .unwrap_or(scan_from);
            let last_hi = (last.span.hi.0 - self.file_start.0) as usize;
            self.blank.blank_range(kw_start..last_hi);
        }
        if let Some(super_class) = &n.super_class {
            super_class.visit_with(self);
        }
        for decorator in &n.decorators {
            decorator.visit_with(self);
        }
        n.body.visit_with(self);
    }

    fn visit_constructor(&mut self, n: &Constructor) {
        let member_start = (n.span.lo.0 - self.file_start.0) as usize;
        let key_start = (n.key.span().lo.0 - self.file_start.0) as usize;
        self.blank_modifiers_in_range(member_start, key_start, false);

        for p in &n.params {
            match p {
                ParamOrTsParamProp::TsParamProp(prop) => prop.visit_with(self),
                ParamOrTsParamProp::Param(param) => {
                    self.blank_param_type_and_optional(&param.pat);
                    param.pat.visit_with(self);
                }
            }
        }
        if let Some(body) = &n.body {
            body.visit_with(self);
        }
    }

    fn visit_class_method(&mut self, n: &ClassMethod) {
        if n.function.body.is_none() {
            // Overload / abstract signature: no runtime code, blank entirely.
            self.blank_statement(n.span);
            return;
        }

        let member_start = (n.span.lo.0 - self.file_start.0) as usize;
        let key_start = (n.key.span().lo.0 - self.file_start.0) as usize;
        let add_semi = matches!(n.key, PropName::Computed(_));
        self.blank_modifiers_in_range(member_start, key_start, add_semi);

        if n.is_optional {
            let pos = (n.key.span().hi.0 - self.file_start.0) as usize;
            self.blank.replace_at(pos, b' ');
        }
        if let PropName::Computed(key) = &n.key {
            key.expr.visit_with(self);
        }
        self.blank_this_param(&n.function.this_param);
        self.blank_type_params(&n.function.type_params);
        self.blank_return_type(&n.function.return_type);
        for param in &n.function.params {
            self.blank_param_type_and_optional(&param.pat);
        }
        n.function.visit_with(self);
    }

    fn visit_class_prop(&mut self, n: &ClassProp) {
        let member_start = (n.span.lo.0 - self.file_start.0) as usize;
        let member_end = (n.span.hi.0 - self.file_start.0) as usize;
        let key_start = (n.key.span().lo.0 - self.file_start.0) as usize;
        let key_end = (n.key.span().hi.0 - self.file_start.0) as usize;

        if n.declare || n.is_abstract {
            self.blank_statement(n.span);
            return;
        }

        for decorator in &n.decorators {
            decorator.visit_with(self);
        }
        let modifiers_start = n
            .decorators
            .last()
            .map(|d| (d.span.hi.0 - self.file_start.0) as usize)
            .unwrap_or(member_start);
        let add_semi = matches!(n.key, PropName::Computed(_));
        self.blank_modifiers_in_range(modifiers_start, key_start, add_semi);

        let marker_end = n
            .type_ann
            .as_ref()
            .map(|ann| (ann.span.lo.0 - self.file_start.0) as usize)
            .or_else(|| {
                n.value
                    .as_ref()
                    .map(|v| (v.span().lo.0 - self.file_start.0) as usize)
            })
            .unwrap_or(member_end);
        if n.is_optional {
            self.blank.blank_first_char_in(key_end, marker_end, b'?');
        }
        if n.definite {
            self.blank.blank_first_char_in(key_end, marker_end, b'!');
        }

        if let PropName::Computed(key) = &n.key {
            key.expr.visit_with(self);
        }
        if let Some(ann) = &n.type_ann {
            self.blank_span(ann.span);
        }
        if let Some(value) = &n.value {
            value.visit_with(self);
        }
    }

    fn visit_call_expr(&mut self, n: &CallExpr) {
        self.blank_type_args(&n.type_args);
        n.visit_children_with(self);
    }

    fn visit_new_expr(&mut self, n: &NewExpr) {
        self.blank_type_args(&n.type_args);
        n.visit_children_with(self);
    }

    fn visit_tagged_tpl(&mut self, n: &TaggedTpl) {
        self.blank_type_args(&n.type_params);
        n.visit_children_with(self);
    }

    fn visit_ts_instantiation(&mut self, n: &TsInstantiation) {
        n.expr.visit_with(self);
        self.blank_span(n.type_args.span);
    }

    fn visit_var_declarator(&mut self, n: &VarDeclarator) {
        if let Pat::Ident(bi) = &n.name {
            if let Some(ann) = &bi.type_ann {
                self.blank_span(ann.span);
            }
            if bi.id.optional {
                let pos = (bi.id.span.hi.0 - self.file_start.0) as usize;
                self.blank.replace_at(pos, b' ');
            }
            if n.definite {
                // `!` sits right after the identifier, before any `: T`.
                let pos = (bi.id.span.hi.0 - self.file_start.0) as usize;
                self.blank.replace_at(pos, b' ');
            }
        }
        n.visit_children_with(self);
    }

    fn visit_arrow_expr(&mut self, n: &ArrowExpr) {
        let whole = self.range(n.span);
        let params_open = self
            .find_byte_forward(whole.start, b'(')
            .unwrap_or(whole.start);
        // Computed before any mutation below, since blanking the type
        // params can overwrite the byte at `params_open` itself.
        let params_close = self.find_matching_close_paren(params_open);

        if let Some(tp) = &n.type_params {
            let tp_range = self.range(tp.span);
            if self.spans_lines(tp_range.start, params_open) {
                // Danger: a line break between `<T>` and `(` would break
                // `async` arrow-function recognition once blanked (no line
                // terminator is allowed there). Move the `(` to right after
                // the generics instead of leaving it on its own line.
                self.blank.blank_range(tp_range.clone());
                self.blank.replace_at(tp_range.start, b'(');
                self.blank.replace_at(params_open, b' ');
            } else {
                self.blank.blank_range(tp_range);
            }
        }

        for pat in &n.params {
            self.blank_param_type_and_optional(pat);
        }

        if let Some(ret) = &n.return_type {
            let ret_range = self.range(ret.span);
            let arrow_pos = self
                .find_bytes_forward(ret_range.end, b"=>")
                .unwrap_or(ret_range.end);
            if self.spans_lines(params_close, arrow_pos) {
                // Danger: a line break between `)` and `=>` is fine for a
                // block-bodied arrow but would let the blanked return type
                // swallow what follows via ASI. Blank through the return
                // type but re-plant `)` as the very last blanked byte, so
                // it sits immediately before `=>`.
                self.blank.blank_range(params_close..ret_range.end);
                self.blank.replace_at(ret_range.end - 1, b')');
            } else {
                self.blank.blank_range(ret_range);
            }
        }

        // Not `visit_children_with`: that would re-visit type_params/
        // return_type through the default recursion and clobber the
        // careful paren-repositioning done above.
        for pat in &n.params {
            pat.visit_with(self);
        }
        n.body.visit_with(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_fixture(input: &str, expected: &str) {
        let actual = strip_types(input).expect("stripping should succeed");
        assert_eq!(actual, expected);
        assert_eq!(
            actual.len(),
            input.len(),
            "output must be the same byte length as input"
        );
        assert_eq!(
            actual.matches('\n').count(),
            input.matches('\n').count(),
            "output must preserve line count"
        );
    }

    #[test]
    fn fixture_a() {
        assert_fixture(include_str!("fixtures/a.ts"), include_str!("fixtures/a.js"));
    }

    #[test]
    fn fixture_b() {
        assert_fixture(include_str!("fixtures/b.ts"), include_str!("fixtures/b.js"));
    }

    #[test]
    fn fixture_arrow_functions() {
        assert_fixture(
            include_str!("fixtures/arrow-functions.ts"),
            include_str!("fixtures/arrow-functions.js"),
        );
    }

    #[test]
    fn fixture_asi() {
        assert_fixture(
            include_str!("fixtures/asi.ts"),
            include_str!("fixtures/asi.js"),
        );
    }

    #[test]
    fn fixture_assertion_precedence_non_errors() {
        assert_fixture(
            include_str!("fixtures/assertion-precedence-non-errors.ts"),
            include_str!("fixtures/assertion-precedence-non-errors.js"),
        );
    }

    #[test]
    fn fixture_decorators() {
        assert_fixture(
            include_str!("fixtures/decorators.ts"),
            include_str!("fixtures/decorators.js"),
        );
    }

    #[test]
    fn fixture_modules() {
        assert_fixture(
            include_str!("fixtures/modules.ts"),
            include_str!("fixtures/modules.js"),
        );
    }

    #[test]
    fn fixture_namespaces() {
        assert_fixture(
            include_str!("fixtures/namespaces.ts"),
            include_str!("fixtures/namespaces.js"),
        );
    }

    #[test]
    fn fixture_parenthetised_types() {
        assert_fixture(
            include_str!("fixtures/parenthetised-types.ts"),
            include_str!("fixtures/parenthetised-types.js"),
        );
    }

    #[test]
    fn interface_is_blanked() {
        let input = "interface Foo { x: number }\nconst y = 1;";
        let out = strip_types(input).unwrap();
        assert!(!out.contains("interface"));
        assert!(out.contains("const y = 1;"));
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn type_alias_is_blanked() {
        let input = "type Foo = number;\nconst y: Foo = 1;";
        let out = strip_types(input).unwrap();
        assert!(!out.contains("type Foo"));
        assert!(!out.contains(": Foo"));
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn generic_function_strips_type_params_and_args() {
        let input = "function id<T>(x: T): T { return x; }\nid<number>(1);";
        let out = strip_types(input).unwrap();
        assert!(!out.contains("<T>"));
        assert!(!out.contains(": T"));
        assert!(!out.contains("<number>"));
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn safe_as_cast_is_blanked() {
        let input = "const x = 1 as number;";
        let out = strip_types(input).unwrap();
        assert!(!out.contains("as number"));
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn const_assertion_is_blanked() {
        // `expr as const` parses to its own `TsConstAssertion` node, distinct
        // from `TsAsExpr` (`expr as SomeType`) — a separate node the erasure
        // visitor must handle, or it's left as unerased TS syntax no JS
        // parser can read.
        let input = "const x = { a: 1 } as const;";
        let out = strip_types(input).unwrap();
        assert!(!out.contains("as const"));
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn declare_function_is_blanked() {
        let input = "declare function foo(): void;\nfoo();";
        let out = strip_types(input).unwrap();
        assert!(!out.contains("declare"));
        assert!(out.contains("foo();"));
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn plain_enum_is_unsupported() {
        let input = "enum Color { Red, Green }";
        let err = strip_types(input).unwrap_err();
        assert!(matches!(err[0], StripError::UnsupportedEnum(_)));
    }

    #[test]
    fn declare_enum_is_blanked() {
        let input = "declare enum Color { Red, Green }";
        let out = strip_types(input).unwrap();
        assert!(out.trim().is_empty());
    }

    #[test]
    fn constructor_parameter_property_is_unsupported() {
        let input = "class Foo { constructor(public x: number) {} }";
        let err = strip_types(input).unwrap_err();
        assert!(matches!(
            err[0],
            StripError::UnsupportedParameterProperty(_)
        ));
    }

    #[test]
    fn legacy_cast_is_unsupported() {
        let input = "const x = <number>1;";
        let err = strip_types(input).unwrap_err();
        assert!(matches!(err[0], StripError::UnsupportedLegacyCast(_)));
    }

    #[test]
    fn import_equals_is_unsupported() {
        let input = "import foo = require('foo');";
        let err = strip_types(input).unwrap_err();
        assert!(matches!(err[0], StripError::UnsupportedImportEquals(_)));
    }

    #[test]
    fn access_modifiers_are_blanked() {
        let input = "class Foo { private readonly x: number = 1; }";
        let out = strip_types(input).unwrap();
        assert!(!out.contains("private"));
        assert!(!out.contains("readonly"));
        assert!(out.contains("x"));
        assert_eq!(out.len(), input.len());
    }
}
