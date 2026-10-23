//! Lua → JavaScript transpiler — canonical implementation.
//!
//! Migrated in Phase 4.3 from `crates/jengine-web/src/lib.rs` and
//! `crates/jengine-editor-viewport/src/lib.rs` (the editor-viewport copy was the
//! more complete of the two and is what's preserved here verbatim).
//!
//! This is a private detail of [`super::JsAdapter`]. The only public surface is
//! [`transpile_lua_to_js`], which the adapter re-exports as
//! `bridge::adapters::js::transpile_lua_to_js` so the existing web/editor-viewport
//! play-mode paths can call it during the P4.4/P4.5 cutover. After Phases 5–6
//! those call sites will go through `JsAdapter::load` directly.
//!
//! Handles the subset of Lua used in jEngine game scripts:
//! - `function name(args)...end` → `function name(args) {...}`
//! - `--` line comments → `//`
//! - `local x = ...` → `var x = ...`
//! - `..` string concat → `+`
//! - `~=` → `!==`
//! - `not` / `and` / `or` → `!` / `&&` / `||`
//! - `elseif` → `else if`, `then` → `{`, `end` → `}`
//! - `for i = a, b do` → `for (var i = a; i <= b; i++) {`
//! - `for _, v in ipairs(t) do` → indexed for-loop
//! - `#table` → `table.length`, `math.*` → `Math.*`, `nil` → `null`
//! - Lua table constructors → JS object/array literals (pre-pass)

// Code is a verbatim port from the legacy in-crate transpilers; style-only clippy
// suggestions (collapsible-if etc.) are intentionally suppressed to keep the diff
// minimal and reviewable against the original.
#![allow(clippy::collapsible_if, clippy::needless_range_loop)]

/// Public entry point: convert Lua source to JavaScript.
///
/// Strips the `script = { ... }` metadata header (if present) before
/// transpiling — the header is valid Lua but produces `var script = ...`
/// in JS which conflicts with the sandbox wrapper.
pub fn transpile_lua_to_js(source: &str) -> String {
    let stripped = super::super::super::metadata::strip_script_header(source);
    lua_to_js(&stripped)
}

/// Adapter-internal alias used by `JsAdapter::load`.
pub(super) fn transpile(source: &str) -> String {
    transpile_lua_to_js(source)
}

fn lua_to_js(source: &str) -> String {
    // Pre-pass: convert Lua table constructors to JS object/array literals.
    // Lua allows several statements on one line separated by `;`, but every
    // rule below matches a whole line - so a trailing `if ... then ... end`
    // after a `;` never fired. Split them onto their own lines first.
    let source = &split_semicolon_statements(source);
    // Rename Lua identifiers that JS reserves. Must happen before any JS is
    // emitted, so the pass only ever sees Lua spelling.
    let source = rename_reserved_identifiers(source);
    // This must run before line-by-line processing because tables can span lines.
    let source = convert_lua_tables(&source);
    // Every rule below matches on a whole statement sitting on one line, so
    // fold wrapped ones (a call whose arguments run over several lines) back
    // together first. Table literals are already handled above; this catches
    // the call that contains them.
    // Move trailing comments onto their own line. Nearly every rule below
    // anchors on how a statement *ends* (`... end`, `... do`, `... then`), so
    // a trailing `--` comment silently stopped the rule from matching and the
    // Lua fell through untranslated.
    let source = hoist_trailing_comments(&source);
    let source = join_continuation_lines(&source);

    let mut lines: Vec<String> = Vec::new();

    for raw_line in source.lines() {
        let mut line = raw_line.to_string();

        // Convert line comments: -- → //
        if let Some(idx) = line.find("--") {
            let before = &line[..idx];
            let single_q = before.matches('\'').count();
            let double_q = before.matches('"').count();
            if single_q % 2 == 0 && double_q % 2 == 0 {
                let comment = &line[idx + 2..];
                line = format!("{}// {}", before, comment.trim_start());
            }
        }

        let trimmed = line.trim();

        // Single-line `local function name(a) BODY end`
        if trimmed.starts_with("local function ")
            && trimmed.ends_with(" end")
            && trimmed.contains(')')
        {
            if let Some(close) = trimmed.find(')') {
                let head = &trimmed[6..close + 1];
                let body = trimmed[close + 1..trimmed.len() - 4].trim();
                let body = convert_inline_body(body);
                let indent = &line[..line.len() - trimmed.len()];
                lines.push(format!("{indent}{head} {{ {body} }}"));
                continue;
            }
        }

        // local function name(args) → function name(args) {
        if trimmed.starts_with("local function ")
            && trimmed.contains('(')
            && trimmed.contains(')')
        {
            let func_part = &trimmed[6..]; // Strip "local " → "function name(args)"
            let indent = &line[..line.len() - trimmed.len()];
            line = format!("{}{} {{", indent, func_part);
            lines.push(line);
            continue;
        }

        // Single-line function: `function name(args) BODY end`
        if trimmed.starts_with("function ")
            && trimmed.ends_with(" end")
            && trimmed.contains('(')
            && trimmed.contains(')')
            && !trimmed.contains('{')
        {
            if let Some(close) = trimmed.find(')') {
                let head = &trimmed[..close + 1];
                let body = trimmed[close + 1..trimmed.len() - 4].trim();
                let body = convert_lua_expr(body);
                let indent = &line[..line.len() - trimmed.len()];
                line = format!("{}{} {{ {} }}", indent, head, body);
                lines.push(line);
                continue;
            }
        }

        // function name(args) → function name(args) {
        if trimmed.starts_with("function ")
            && trimmed.contains('(')
            && trimmed.contains(')')
            && !trimmed.contains('{')
        {
            let indent = &line[..line.len() - trimmed.len()];
            // `function M.foo(a)` / `function M:foo(a)` — a declaration form
            // JS does not have. Emit the assignment twin instead; a colon
            // definition also gets Lua's implicit `self` as a real parameter.
            if let Some(open) = trimmed.find('(') {
                let name = trimmed[9..open].trim();
                if let Some((table, method)) = name
                    .split_once(':')
                    .map(|(t, m)| (t, m))
                    .filter(|_| !name.contains('.'))
                {
                    let args = trimmed[open + 1..trimmed.rfind(')').unwrap_or(open + 1)].trim();
                    let args = if args.is_empty() {
                        "self".to_string()
                    } else {
                        format!("self, {args}")
                    };
                    line = format!("{indent}{table}.{method} = function({args}) {{");
                    lines.push(line);
                    continue;
                }
                if name.contains('.') {
                    let args = &trimmed[open..];
                    let args = args.trim_end();
                    line = format!("{indent}{name} = function{args} {{");
                    lines.push(line);
                    continue;
                }
            }
            line = line.replace(trimmed, &format!("{} {{", trimmed));
            lines.push(line);
            continue;
        }

        // Phase D — assignment-form function definitions:
        //   `M.foo = function(a, b) BODY end` (single-line)
        //   `M.foo = function(a, b)`          (multi-line head)
        // These are the canonical idiom for module exports
        // (`local M = {}; M.foo = function(...) ... end; return M`).
        // We emit `M.foo = function(a, b) { BODY };` and `M.foo = function(a, b) {`
        // respectively; the existing `end → }` handler closes the multi-line form.
        if trimmed.contains(" = function(") || trimmed.contains(" = function (") {
            if let Some(eq_fn_pos) = trimmed.find("= function") {
                let lhs = trimmed[..eq_fn_pos].trim();
                let after = &trimmed[eq_fn_pos + 1..]; // " function(...)..."
                if !lhs.is_empty()
                    && !lhs.starts_with("local ")
                    && !lhs.starts_with("var ")
                    && lhs.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.')
                {
                    let after_trim = after.trim_start();
                    if let Some(open) = after_trim.find('(') {
                        if let Some(close) = after_trim.find(')') {
                            if close > open {
                                let args = &after_trim[open + 1..close];
                                let tail = after_trim[close + 1..].trim();
                                let indent = &line[..line.len() - trimmed.len()];
                                if let Some(body) = tail.strip_suffix(" end") {
                                    // Single-line.
                                    let body = convert_lua_expr(body.trim());
                                    line = format!(
                                        "{}{} = function({}) {{ {} }};",
                                        indent, lhs, args, body
                                    );
                                    lines.push(line);
                                    continue;
                                } else if tail.is_empty() {
                                    // Multi-line head; existing `end → }` closes it.
                                    line = format!(
                                        "{}{} = function({}) {{",
                                        indent, lhs, args
                                    );
                                    lines.push(line);
                                    continue;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Single-line if/elseif chain: `if C1 then B1 elseif C2 then B2 [else B3] end`
        if trimmed.starts_with("if ")
            && trimmed.ends_with(" end")
            && trimmed.contains(" then ")
            && trimmed.contains(" elseif ")
        {
            let indent = &line[..line.len() - trimmed.len()];
            if let Some(js) = single_line_if_chain(&trimmed[..trimmed.len() - 4], indent) {
                lines.push(js);
                continue;
            }
        }

        // Single-line if: if ... then ... end → if (...) { ... }
        if trimmed.starts_with("if ")
            && trimmed.ends_with(" end")
            && trimmed.contains(" then ")
            && !trimmed.contains(" else ")
        {
            if let Some(then_pos) = trimmed.find(" then ") {
                let cond = trimmed[3..then_pos].trim();
                let cond = convert_lua_expr(cond);
                let body = trimmed[then_pos + 6..trimmed.len() - 4].trim();
                let body = convert_inline_body(body);
                let indent = &line[..line.len() - trimmed.len()];
                line = format!("{}if ({}) {{ {} }}", indent, cond, body);
                lines.push(line);
                continue;
            }
        }

        // if ... then → if (...) {
        if trimmed.starts_with("if ") && trimmed.ends_with("then") {
            let cond = &trimmed[3..trimmed.len() - 4].trim();
            let cond = convert_lua_expr(cond);
            let indent = &line[..line.len() - trimmed.len()];
            line = format!("{}if ({}) {{", indent, cond);
            lines.push(line);
            continue;
        }

        // Single-line if/else: `if COND then BODY1 else BODY2 end`
        if trimmed.starts_with("if ")
            && trimmed.ends_with(" end")
            && trimmed.contains(" then ")
            && trimmed.contains(" else ")
        {
            if let (Some(then_pos), Some(else_pos)) =
                (trimmed.find(" then "), trimmed.rfind(" else "))
            {
                if else_pos > then_pos {
                    let cond = trimmed[3..then_pos].trim();
                    let cond = convert_lua_expr(cond);
                    let body1 = trimmed[then_pos + 6..else_pos].trim();
                    let body1 = convert_lua_expr(body1);
                    let body2 = trimmed[else_pos + 6..trimmed.len() - 4].trim();
                    let body2 = convert_lua_expr(body2);
                    let indent = &line[..line.len() - trimmed.len()];
                    line = format!(
                        "{}if ({}) {{ {} }} else {{ {} }}",
                        indent, cond, body1, body2
                    );
                    lines.push(line);
                    continue;
                }
            }
        }

        // Inline-body if: `if COND then BODY` (no trailing `end`)
        if trimmed.starts_with("if ") && trimmed.contains(" then ") && !trimmed.ends_with(" end") {
            if let Some(then_pos) = trimmed.find(" then ") {
                let cond = trimmed[3..then_pos].trim();
                let cond = convert_lua_expr(cond);
                let body = trimmed[then_pos + 6..].trim();
                let body = convert_lua_expr(body);
                let indent = &line[..line.len() - trimmed.len()];
                line = format!("{}if ({}) {{ {}", indent, cond, body);
                lines.push(line);
                continue;
            }
        }

        // Single-line tail of a multi-line if: `elseif COND then BODY end`
        if trimmed.starts_with("elseif ")
            && trimmed.ends_with(" end")
            && trimmed.contains(" then ")
        {
            if let Some(then_pos) = trimmed.find(" then ") {
                let cond = convert_lua_expr(trimmed[7..then_pos].trim());
                let body = trimmed[then_pos + 6..trimmed.len() - 4].trim();
                let body = convert_inline_body(body);
                let indent = &line[..line.len() - trimmed.len()];
                lines.push(format!("{indent}}} else if ({cond}) {{ {body} }}"));
                continue;
            }
        }

        // elseif ... then → } else if (...) {
        if trimmed.starts_with("elseif ") && trimmed.ends_with("then") {
            let cond = &trimmed[7..trimmed.len() - 4].trim();
            let cond = convert_lua_expr(cond);
            let indent = &line[..line.len() - trimmed.len()];
            line = format!("{}}} else if ({}) {{", indent, cond);
            lines.push(line);
            continue;
        }

        // Inline-body elseif: `elseif COND then BODY` (no trailing `end`)
        if trimmed.starts_with("elseif ")
            && trimmed.contains(" then ")
            && !trimmed.ends_with(" end")
        {
            if let Some(then_pos) = trimmed.find(" then ") {
                let cond = trimmed[7..then_pos].trim();
                let cond = convert_lua_expr(cond);
                let body = trimmed[then_pos + 6..].trim();
                let body = convert_lua_expr(body);
                let indent = &line[..line.len() - trimmed.len()];
                line = format!("{}}} else if ({}) {{ {}", indent, cond, body);
                lines.push(line);
                continue;
            }
        }

        // else → } else {
        if trimmed == "else" {
            let indent = &line[..line.len() - trimmed.len()];
            line = format!("{}}} else {{", indent);
            lines.push(line);
            continue;
        }

        // Single-line tail: `else BODY end` closes a multi-line if.
        if trimmed.starts_with("else ")
            && trimmed.ends_with(" end")
            && !trimmed.starts_with("elseif ")
        {
            let body = convert_inline_body(trimmed[5..trimmed.len() - 4].trim());
            let indent = &line[..line.len() - trimmed.len()];
            lines.push(format!("{indent}}} else {{ {body} }}"));
            continue;
        }

        // Inline-body else: `else BODY`
        if trimmed.starts_with("else ") && !trimmed.starts_with("elseif ") {
            let body = trimmed[5..].trim();
            let body = convert_lua_expr(body);
            let indent = &line[..line.len() - trimmed.len()];
            line = format!("{}}} else {{ {}", indent, body);
            lines.push(line);
            continue;
        }

        // Single-line loop: `for HEADER do BODY end`
        if trimmed.starts_with("for ")
            && trimmed.ends_with(" end")
            && trimmed.contains(" do ")
        {
            if let Some(do_pos) = trimmed.rfind(" do ") {
                let header = trimmed[4..do_pos].trim();
                let body = trimmed[do_pos + 4..trimmed.len() - 4].trim();
                let indent = &line[..line.len() - trimmed.len()];
                if let Some(head_js) = for_header_to_js(header, indent) {
                    let body = convert_inline_body(body);
                    lines.push(format!("{head_js} {body} }}"));
                    continue;
                }
            }
        }

        // for i = a, b do → for (var i = a; i <= b; i++) {
        if trimmed.starts_with("for ") && trimmed.ends_with("do") {
            let body = &trimmed[4..trimmed.len() - 2].trim();
            let indent = &line[..line.len() - trimmed.len()];
            if let Some(head_js) = for_header_to_js(body, indent) {
                lines.push(head_js);
                continue;
            }

            line = format!("{}for ({}) {{", indent, body);
            lines.push(line);
            continue;
        }

        // while ... do → while (...) {
        if trimmed.starts_with("while ") && trimmed.ends_with("do") {
            let cond = &trimmed[6..trimmed.len() - 2].trim();
            let cond = convert_lua_expr(cond);
            let indent = &line[..line.len() - trimmed.len()];
            line = format!("{}while ({}) {{", indent, cond);
            lines.push(line);
            continue;
        }

        // end → }
        if trimmed == "end" {
            let indent = &line[..line.len() - trimmed.len()];
            line = format!("{}}}", indent);
            lines.push(line);
            continue;
        }

        // local x = ... → var x = ...
        // local x, y = func(...) → var [x, y] = func(...)
        // local x, y = a, b      → var [x, y] = [a, b]
        if trimmed.starts_with("local ") {
            let (local_code, local_comment) = split_trailing_comment(trimmed);
            if let Some(eq_pos) = local_code.find('=') {
                let vars_part = local_code[6..eq_pos].trim();
                if vars_part.contains(',') {
                    let indent = &line[..line.len() - trimmed.len()];
                    let rhs = local_code[eq_pos + 1..].trim();
                    let rhs_parts = split_top_level_commas(rhs);
                    let expr_part = if rhs_parts.len() > 1 {
                        let converted: Vec<String> = rhs_parts
                            .into_iter()
                            .map(|p| convert_lua_expr(p.trim()))
                            .collect();
                        format!("[{}]", converted.join(", "))
                    } else {
                        convert_lua_expr(rhs)
                    };
                    line = format!(
                        "{}var [{}] = {}{}",
                        indent, vars_part, expr_part, local_comment
                    );
                    lines.push(line);
                    continue;
                }
            }
            line = line.replacen("local ", "var ", 1);
        }

        let trimmed = line.trim();

        // Lua multi-value return: `return a, b`.
        // In JS, `return a, b` is the comma operator and returns only `b`,
        // so wrap top-level multi-values into an array to preserve semantics.
        // A trailing `// ...` (already converted from `--`) has to come off
        // before the values are wrapped, or the `]` lands inside the comment
        // and the array never closes.
        let (ret_code, ret_comment) = split_trailing_comment(trimmed);
        if let Some(exprs) = ret_code.strip_prefix("return ") {
            let parts = split_top_level_commas(exprs);
            if parts.len() > 1 {
                let indent = &line[..line.len() - trimmed.len()];
                let converted: Vec<String> =
                    parts.into_iter().map(|p| convert_lua_expr(p.trim())).collect();
                line = format!(
                    "{}return [{}]{}",
                    indent,
                    converted.join(", "),
                    ret_comment
                );
                lines.push(line);
                continue;
            }
        }

        // Non-local multi-assign: `a, b = x, y` or `a, b = func(...)`.
        // In JS, the bare comma form is parsed as the comma operator
        // (`a, (b = x), y`), which only assigns `b` and silently
        // discards the rest — a notorious source of "values stay 0"
        // bugs ported from Lua. Promote to array destructuring so
        // semantics line up with Lua's parallel assignment. We only
        // match when both sides have multiple comma-separated parts
        // OR the LHS is multiple identifiers and the RHS is a single
        // call expression returning a tuple (mirrors `local x, y =
        // get_position(...)`). Lines that already start with reserved
        // keywords (`if`, `for`, `return`, `local`, `var`, etc.) are
        // skipped — those have dedicated handlers above.
        if !trimmed.is_empty()
            && !trimmed.starts_with("local ")
            && !trimmed.starts_with("var ")
            && !trimmed.starts_with("if ")
            && !trimmed.starts_with("elseif ")
            && !trimmed.starts_with("for ")
            && !trimmed.starts_with("while ")
            && !trimmed.starts_with("return")
            && !trimmed.starts_with("function ")
            && let Some(eq_pos) = find_top_level_assign(trimmed)
        {
            let lhs = trimmed[..eq_pos].trim();
            let rhs = trimmed[eq_pos + 1..].trim_end_matches(';').trim();
            let lhs_parts = split_top_level_commas(lhs);
            let (rhs, assign_comment) = split_trailing_comment(rhs);
            if lhs_parts.len() > 1
                && lhs_parts.iter().all(|p| is_simple_lvalue(p.trim()))
            {
                let indent = &line[..line.len() - trimmed.len()];
                let lhs_joined = lhs_parts
                    .iter()
                    .map(|p| p.trim())
                    .collect::<Vec<_>>()
                    .join(", ");
                let rhs_parts = split_top_level_commas(rhs);
                let rhs_expr = if rhs_parts.len() > 1 {
                    let converted: Vec<String> = rhs_parts
                        .into_iter()
                        .map(|p| convert_lua_expr(p.trim()))
                        .collect();
                    format!("[{}]", converted.join(", "))
                } else {
                    convert_lua_expr(rhs)
                };
                // Leading `;` defends against ASI hazards if the
                // previous emitted line ended without one.
                line = format!(
                    "{};[{}] = {}{}",
                    indent, lhs_joined, rhs_expr, assign_comment
                );
                lines.push(line);
                continue;
            }
        }

        // General expression conversions
        line = convert_lua_expr(&line);

        lines.push(line);
    }

    lines.join("\n")
}

/// Convert a statement list that shares one line - the body of an inline
/// `then`, loop, or function.
///
/// `convert_lua_expr` alone is not enough here: a body like
/// `local x, y = f(); set_position(e, x, y)` is two *statements*, and the
/// `local` needs the same declaration handling the line-level rules give it.
fn convert_inline_body(body: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut rest = body;
    loop {
        match top_level_semicolon(rest) {
            Some(pos) => {
                parts.push(rest[..pos].trim().to_string());
                rest = &rest[pos + 1..];
            }
            None => {
                parts.push(rest.trim().to_string());
                break;
            }
        }
    }
    parts
        .iter()
        .filter(|p| !p.is_empty())
        .map(|p| convert_inline_statement(p))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Convert one statement from an inline body.
fn convert_inline_statement(stmt: &str) -> String {
    // An inline body can hold its own single-line conditional:
    //   for k, v in pairs(t) do if v > 0 then t[k] = 0 end end
    if stmt.starts_with("if ") && stmt.ends_with(" end") && stmt.contains(" then ") {
        let head = &stmt[..stmt.len() - 4];
        if stmt.contains(" elseif ")
            && let Some(js) = single_line_if_chain(head, "")
        {
            return js;
        }
        if let Some(then_pos) = stmt.find(" then ") {
            let cond = convert_lua_expr(stmt[3..then_pos].trim());
            let body = convert_inline_body(stmt[then_pos + 6..stmt.len() - 4].trim());
            return format!("if ({cond}) {{ {body} }}");
        }
    }
    if let Some(rest) = stmt.strip_prefix("local ") {
        if let Some(eq) = rest.find('=') {
            let vars = rest[..eq].trim();
            let rhs = rest[eq + 1..].trim();
            if vars.contains(',') {
                let rhs_parts = split_top_level_commas(rhs);
                let expr = if rhs_parts.len() > 1 {
                    format!(
                        "[{}]",
                        rhs_parts
                            .into_iter()
                            .map(|p| convert_lua_expr(p.trim()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                } else {
                    convert_lua_expr(rhs)
                };
                return format!("var [{vars}] = {expr}");
            }
            return format!("var {vars} = {}", convert_lua_expr(rhs));
        }
        return format!("var {}", convert_lua_expr(rest));
    }
    separate_inline_statements(&convert_lua_expr(stmt))
}

/// Move a trailing `--` comment onto its own line above the code.
///
/// Comment placement in generated JS is cosmetic; matching rules reliably is
/// not. Blank-code lines (a whole-line comment) are left as they are.
fn hoist_trailing_comments(source: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for raw in source.lines() {
        let (code, comment) = split_lua_comment(raw);
        if comment.is_empty() || code.trim().is_empty() {
            out.push(raw.to_string());
            continue;
        }
        let indent: String = raw.chars().take_while(|c| c.is_whitespace()).collect();
        out.push(format!("{indent}{}", comment.trim_end()));
        out.push(code.trim_end().to_string());
    }
    out.join("
")
}

/// Put statements that share a line via `;` onto their own lines.
///
/// Only splits lines that *start* with a simple statement: a line opening
/// with `if` / `for` / `while` / `function` may carry a `;` inside a block
/// body, where splitting would separate the body from its `end`.
fn split_semicolon_statements(source: &str) -> String {
    const BLOCK_STARTERS: &[&str] = &[
        "if ", "for ", "while ", "function ", "local function ", "elseif ", "else", "repeat",
        "do", "return",
    ];
    let mut out: Vec<String> = Vec::new();
    for raw in source.lines() {
        let (code, comment) = split_lua_comment(raw);
        let trimmed = code.trim();
        if BLOCK_STARTERS.iter().any(|k| trimmed.starts_with(k)) {
            out.push(raw.to_string());
            continue;
        }
        let Some(pos) = top_level_semicolon(code) else {
            out.push(raw.to_string());
            continue;
        };
        let indent: String = raw.chars().take_while(|c| c.is_whitespace()).collect();
        let head = code[..pos].trim_end();
        let tail = code[pos + 1..].trim();
        if tail.is_empty() {
            out.push(raw.to_string());
            continue;
        }
        out.push(format!("{indent}{head}"));
        // The remainder may hold further `;`-separated statements.
        let rest = format!("{indent}{tail}{comment}");
        out.push(split_semicolon_statements(&rest));
    }
    out.join("\n")
}

/// Byte offset of the first `;` that is not inside a string or brackets.
fn top_level_semicolon(code: &str) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'(' | b'[' | b'{' if !in_single && !in_double => depth += 1,
            b')' | b']' | b'}' if !in_single && !in_double => depth -= 1,
            b';' if !in_single && !in_double && depth == 0 => return Some(i),
            _ => {}
        }
        i += 1;
    }
    None
}

/// Build an `if / else if / else` chain from a single-line Lua conditional.
///
/// `body` is everything from `if` up to (not including) the trailing `end`.
/// Returns `None` when the shape does not parse, so the caller can fall
/// through to the simpler rules.
fn single_line_if_chain(body: &str, indent: &str) -> Option<String> {
    // Split into (condition, branch-body) pairs plus an optional else body.
    let mut branches: Vec<(String, String)> = Vec::new();
    let mut else_body: Option<String> = None;

    let mut rest = body.strip_prefix("if ")?.to_string();
    loop {
        let then_pos = rest.find(" then ")?;
        debug_assert!(!rest.starts_with("if "), "branch condition kept its keyword");
        let cond = rest[..then_pos].trim().to_string();
        let after = rest[then_pos + 6..].to_string();

        // The branch runs until the next `elseif` / `else` at this level.
        let elseif_pos = after.find(" elseif ");
        let else_pos = after.find(" else ");
        let cut = match (elseif_pos, else_pos) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };
        match cut {
            None => {
                branches.push((cond, after.trim().to_string()));
                break;
            }
            Some(pos) => {
                branches.push((cond, after[..pos].trim().to_string()));
                let tail = &after[pos..];
                if let Some(next) = tail.strip_prefix(" elseif ") {
                    // `next` already starts at the condition - re-adding the
                    // `if ` here is what produced `else if (if cond)`.
                    rest = next.to_string();
                    continue;
                }
                else_body = tail.strip_prefix(" else ").map(|b| b.trim().to_string());
                break;
            }
        }
    }

    let mut out = String::new();
    for (i, (cond, branch)) in branches.iter().enumerate() {
        let cond_js = convert_lua_expr(cond);
        let body_js = convert_inline_body(branch);
        if i == 0 {
            out.push_str(&format!("{indent}if ({cond_js}) {{ {body_js} }}"));
        } else {
            out.push_str(&format!(" else if ({cond_js}) {{ {body_js} }}"));
        }
    }
    if let Some(else_body) = else_body {
        let body_js = convert_inline_body(&else_body);
        out.push_str(&format!(" else {{ {body_js} }}"));
    }
    Some(out)
}

/// JS keywords that are ordinary identifiers in Lua.
///
/// A script is free to name a local `new` or a parameter `class`; emitting
/// those verbatim produces JS that will not parse. Renaming with a trailing
/// underscore keeps the script self-consistent.
const JS_RESERVED_IDENTS: &[&str] = &[
    "new", "class", "const", "let", "var", "typeof", "instanceof", "delete", "void", "with",
    "yield", "enum", "export", "import", "case", "switch", "default", "catch", "finally", "try",
    "throw", "debugger", "this", "await", "static", "extends", "interface", "implements",
    "package", "private", "protected", "public",
];

/// Rename JS-reserved identifiers so the emitted code parses.
///
/// Occurrences after a `.` or `:` are left alone — those are property names
/// on a table that may have come from the host (a data row with a `new`
/// field keeps that field), and renaming them would break the lookup.
/// String contents and comments are untouched.
fn rename_reserved_identifiers(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for (seg, is_str) in split_strings(source) {
        if is_str {
            out.push_str(&seg);
            continue;
        }
        let bytes = seg.as_bytes();
        let mut i = 0usize;
        while i < seg.len() {
            // Skip whole `--` comments verbatim.
            if bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                let end = seg[i..].find('\n').map(|n| i + n).unwrap_or(seg.len());
                out.push_str(&seg[i..end]);
                i = end;
                continue;
            }
            let matched = JS_RESERVED_IDENTS.iter().find(|kw| {
                seg[i..].starts_with(**kw) && is_word_boundary(bytes, i, kw.len())
            });
            match matched {
                Some(kw)
                    if !(i > 0 && (bytes[i - 1] == b'.' || bytes[i - 1] == b':')) =>
                {
                    out.push_str(kw);
                    out.push('_');
                    i += kw.len();
                }
                _ => {
                    let c = seg[i..].chars().next().unwrap();
                    out.push(c);
                    i += c.len_utf8();
                }
            }
        }
    }
    out
}

/// Translate a Lua pattern literal into a JS regex literal source string.
///
/// Only the overlap that game scripts actually use: character classes and
/// literals are already common to both, so this converts Lua's `%` escapes
/// (`%d`, `%s`, `%w`, `%a`, `%.`) to their regex spelling. Anchors,
/// captures and quantifiers pass through. Lua-only constructs (`%b`, `%f`,
/// `-` as a lazy quantifier) are NOT translated.
fn lua_pattern_to_regex(pattern_literal: &str) -> String {
    let quote = pattern_literal.chars().next().unwrap_or('"');
    if quote != '"' && quote != '\'' {
        // Not a literal (a variable holding a pattern) - pass through and
        // let the regex engine take it as-is.
        return pattern_literal.to_string();
    }
    let inner = &pattern_literal[1..pattern_literal.len().saturating_sub(1)];
    let mut out = String::with_capacity(inner.len() + 2);
    let bytes = inner.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 1 < bytes.len() {
            let c = bytes[i + 1] as char;
            let mapped = match c {
                'd' => "\\d".to_string(),
                'D' => "\\D".to_string(),
                's' => "\\s".to_string(),
                'S' => "\\S".to_string(),
                'w' => "\\w".to_string(),
                'W' => "\\W".to_string(),
                'a' => "[A-Za-z]".to_string(),
                'l' => "[a-z]".to_string(),
                'u' => "[A-Z]".to_string(),
                'x' => "[0-9A-Fa-f]".to_string(),
                other => format!("\\{other}"),
            };
            out.push_str(&mapped);
            i += 2;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    format!("\"{}\"", out.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Convert a `for` header (everything between `for` and `do`) into the JS
/// loop opening, or `None` when the shape is not one we rewrite.
///
/// Shared by the multi-line form (`for ... do` on its own line) and the
/// single-line form (`for ... do BODY end`), so both accept exactly the
/// same header syntax.
fn for_header_to_js(header: &str, indent: &str) -> Option<String> {
    // ipairs: for _, v in ipairs(t) do
    if header.contains("ipairs(") {
        if let Some(cap) = parse_ipairs_for(header) {
            // 1-based to match Lua semantics. Arrays returned from
            // script-facing APIs are wrapped in a Lua-style 1-indexed Proxy
            // so `t[1]` is the first element; this loop uses the same
            // convention.
            //
            // The iterated expression is raw Lua spliced into two postfix
            // positions, so it needs operator conversion *and* parentheses
            // when it is not a plain path — without them `ipairs(t or {})`
            // became `t || [].length`.
            let table = convert_lua_expr(&cap.table);
            let table = if is_simple_postfix(&table) {
                table
            } else {
                format!("({table})")
            };
            return Some(format!(
                "{}for (var __i_{2} = 1; __i_{2} <= {3}.length; __i_{2}++) {{ var {1} = {3}[__i_{2}];",
                indent, cap.val_var, cap.idx_var, table,
            ));
        }
    }

    // pairs: `for k, v in pairs(t) do` / `for k in pairs(t) do`
    if let Some(open) = header.find("pairs(") {
        // `ipairs(` also ends in `pairs(`; that form is handled above, so
        // only treat this as `pairs` when the call really starts here.
        let is_plain_pairs = open == 0 || !header[..open].ends_with('i');
        if is_plain_pairs
            && let Some(in_pos) = header.find(" in ")
        {
            let vars: Vec<&str> = header[..in_pos].split(',').map(|v| v.trim()).collect();
            let call = header[in_pos + 4..].trim();
            if let Some(inner) = call
                .strip_prefix("pairs(")
                .and_then(|r| r.strip_suffix(')'))
            {
                let table = convert_lua_expr(inner);
                let table = if is_simple_postfix(&table) {
                    table
                } else {
                    format!("({table})")
                };
                let key = vars.first().copied().unwrap_or("_k");
                let mut out = format!("{indent}for (var {key} in {table}) {{");
                if let Some(val) = vars.get(1) {
                    out.push_str(&format!(" var {val} = {table}[{key}];"));
                }
                return Some(out);
            }
        }
    }

    // `for x in string.gmatch(s, pattern) do` - iterate regex matches.
    if let Some(in_pos) = header.find(" in ") {
        let call = header[in_pos + 4..].trim();
        if let Some(args) = call
            .strip_prefix("string.gmatch(")
            .and_then(|r| r.strip_suffix(')'))
        {
            let parts = split_top_level_commas(args);
            if parts.len() == 2 {
                let subject = convert_lua_expr(parts[0].trim());
                let pattern = parts[1].trim();
                let regex = lua_pattern_to_regex(pattern);
                let var = header[..in_pos].trim();
                return Some(format!(
                    "{indent}for (var {var} of (String({subject}).match(new RegExp({regex}, \"g\")) || [])) {{"
                ));
            }
        }
    }

    // Numeric for: for i = start, stop[, step] do
    if let Some(eq_pos) = header.find('=') {
        let var_name = header[..eq_pos].trim();
        let rest = header[eq_pos + 1..].trim();
        let parts: Vec<&str> = split_top_level_commas(rest)
            .into_iter()
            .map(|s| s.trim())
            .collect();
        if parts.len() >= 2 {
            let start = convert_lua_expr(parts[0]);
            let stop = convert_lua_expr(parts[1]);
            return Some(if parts.len() >= 3 {
                let step = convert_lua_expr(parts[2]);
                format!(
                    "{}for (var {} = {}; {} <= {}; {} += {}) {{",
                    indent, var_name, start, var_name, stop, var_name, step
                )
            } else {
                format!(
                    "{}for (var {} = {}; {} <= {}; {}++) {{",
                    indent, var_name, start, var_name, stop, var_name
                )
            });
        }
    }

    None
}

/// Split a line into code and any trailing `//` comment (comment markers
/// inside string literals do not count).
///
/// Rules that append to the end of a statement have to put their punctuation
/// before the comment, not after it.
fn split_trailing_comment(line: &str) -> (&str, &str) {
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'/' if !in_single && !in_double && bytes[i + 1] == b'/' => {
                return (line[..i].trim_end(), &line[i..]);
            }
            _ => {}
        }
        i += 1;
    }
    (line, "")
}

/// Join lines that leave a bracket open into the line that closes it.
///
/// The transpiler is line-based, so a statement split across lines — say a
/// `for _, v in ipairs({ ... }) do` whose list wraps — never matches its
/// rule and passes through as raw Lua. Counting only brackets outside
/// strings and comments, any line that ends with an unclosed `(`, `[` or `{`
/// absorbs the next one. Comments on the absorbed lines are dropped: kept,
/// they would comment out everything joined after them.
fn join_continuation_lines(source: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut pending = String::new();
    let mut depth = 0i32;

    for raw in source.lines() {
        let (code, _comment) = split_lua_comment(raw);
        if pending.is_empty() {
            depth = bracket_delta(code);
            if depth > 0 || continues_onto_next_line(code) {
                // Comments are dropped from a line that opens a fold: once
                // the following lines sit behind it, a `--` here would
                // comment them out.
                pending = code.trim_end().to_string();
                continue;
            }
            out.push(raw.to_string());
            continue;
        }
        pending.push(' ');
        pending.push_str(code.trim());
        depth += bracket_delta(code);
        if depth <= 0 && !continues_onto_next_line(&pending) {
            depth = 0;
            out.push(std::mem::take(&mut pending));
        }
    }
    if !pending.is_empty() {
        out.push(pending);
    }
    out.join("\n")
}

/// True when a line of code is unfinished and the statement carries on.
///
/// Covers the two bracket-free ways game scripts wrap: a dangling binary
/// operator, and an `if` / `elseif` whose `then` has not arrived yet.
fn continues_onto_next_line(code: &str) -> bool {
    if ends_with_binary_operator(code) {
        return true;
    }
    let t = code.trim_start();
    (t.starts_with("if ") || t.starts_with("elseif ")) && !code.contains(" then")
}

/// True when a line of code ends mid-expression, so the statement carries
/// on to the next line.
fn ends_with_binary_operator(code: &str) -> bool {
    let t = code.trim_end();
    if t.is_empty() {
        return false;
    }
    for op in ["..", "==", "~=", "<=", ">=", "and", "or", "not"] {
        if let Some(head) = t.strip_suffix(op) {
            // Guard against an identifier that merely ends in those letters
            // (`error`, `for`, `cannot`).
            let boundary = head
                .chars()
                .last()
                .is_none_or(|c| !(c.is_ascii_alphanumeric() || c == '_'));
            if !op.chars().next().unwrap().is_ascii_alphabetic() || boundary {
                return true;
            }
        }
    }
    matches!(
        t.chars().last(),
        Some('+') | Some('-') | Some('*') | Some('/') | Some(',') | Some('=') | Some('<')
            | Some('>')
    )
}

/// Split a raw Lua line into code and its trailing `--` comment.
fn split_lua_comment(line: &str) -> (&str, &str) {
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0usize;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'-' if !in_single && !in_double && bytes[i + 1] == b'-' => {
                return (&line[..i], &line[i..]);
            }
            _ => {}
        }
        i += 1;
    }
    (line, "")
}

/// Net bracket depth of a code fragment, ignoring string contents.
fn bracket_delta(code: &str) -> i32 {
    let mut depth = 0i32;
    let mut in_single = false;
    let mut in_double = false;
    let bytes = code.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'(' | b'[' | b'{' if !in_single && !in_double => depth += 1,
            b')' | b']' | b'}' if !in_single && !in_double => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    depth
}

/// True when `expr` is a bare path (`items`, `state.list`, `t[0]`) and can
/// take a postfix `.length` / `[i]` without parentheses.
fn is_simple_postfix(expr: &str) -> bool {
    let e = expr.trim();
    let mut chars = e.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' || c == '$' => {}
        _ => return false,
    }
    e.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.' | '[' | ']'))
}

/// Insert `;` between statements Lua allows to share a line.
///
/// `if id == iid then idx = i - 1 break end` is legal Lua; the JS twin
/// `idx = i - 1 break` is a syntax error, and automatic semicolon insertion
/// cannot rescue it because there is no newline. Only `break` and `return`
/// are handled — they are the keywords game scripts routinely tack onto the
/// end of a single-line `then` body. A separator is added only after
/// something that ends an expression, so `else return x` and `{ return x`
/// (where a `;` would change the meaning or read oddly) are left alone.
fn separate_inline_statements(body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 8);
    for (seg, is_str) in split_strings(body) {
        if is_str {
            out.push_str(&seg);
            continue;
        }
        let bytes = seg.as_bytes();
        let mut i = 0usize;
        while i < seg.len() {
            let rest = &seg[i..];
            let kw = ["break", "return"]
                .into_iter()
                .find(|k| rest.starts_with(k) && is_word_boundary(seg.as_bytes(), i, k.len()));
            let Some(kw) = kw else {
                out.push(seg[i..].chars().next().unwrap());
                i += seg[i..].chars().next().unwrap().len_utf8();
                continue;
            };
            if needs_statement_separator(&out) {
                while out.ends_with(' ') {
                    out.pop();
                }
                out.push_str("; ");
            }
            out.push_str(kw);
            i += kw.len();
            let _ = bytes;
        }
    }
    out
}

/// Whether the keyword at `pos..pos+len` stands alone rather than being part
/// of a longer identifier.
fn is_word_boundary(bytes: &[u8], pos: usize, len: usize) -> bool {
    let before_ok = pos == 0 || !is_ident_byte(bytes[pos - 1]);
    let after = pos + len;
    let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
    before_ok && after_ok
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'$'
}

/// True when what has been emitted so far ends an expression, so a following
/// statement keyword needs a separator.
fn needs_statement_separator(emitted: &str) -> bool {
    let trimmed = emitted.trim_end();
    let Some(last) = trimmed.chars().last() else {
        return false;
    };
    if !(is_ident_byte(last as u8) || matches!(last, ')' | ']' | '"' | '\'')) {
        return false;
    }
    // `else return x` / `do break` must not gain a `;` — that would detach
    // the statement from its branch.
    let last_word: String = trimmed
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    !matches!(last_word.as_str(), "else" | "do" | "then" | "return")
}

fn convert_lua_expr(s: &str) -> String {
    let segments = split_strings(s);
    let mut out_total = String::with_capacity(s.len());
    for (seg, is_str) in segments {
        if is_str {
            out_total.push_str(&seg);
            continue;
        }
        out_total.push_str(&convert_code_segment(&seg));
    }
    // #expr -> expr.length must run on the fully reassembled string so that
    // call expressions like `#find_by_tag("enemy")` (which split_strings
    // breaks across segments) match correctly.
    apply_hash_length(&out_total)
}

fn apply_hash_length(input: &str) -> String {
    let mut out = input.to_string();
    while let Some(hash_pos) = out.find('#') {
        let after = &out[hash_pos + 1..];
        let bytes = after.as_bytes();
        if bytes.is_empty() || !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
            break;
        }
        let mut end = 1usize;
        while end < bytes.len()
            && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
        {
            end += 1;
        }
        // Greedily consume call/index/field chain: ( ... ), [ ... ], .ident
        loop {
            if end >= bytes.len() {
                break;
            }
            match bytes[end] {
                b'(' | b'[' => {
                    let open = bytes[end] as char;
                    let close = if open == '(' { ')' } else { ']' };
                    let mut depth = 1i32;
                    let mut p = end + 1;
                    let mut in_str: Option<u8> = None;
                    while p < bytes.len() && depth > 0 {
                        let c = bytes[p];
                        if let Some(q) = in_str {
                            if c == b'\\' {
                                p += 2;
                                continue;
                            }
                            if c == q {
                                in_str = None;
                            }
                        } else if c == b'"' || c == b'\'' {
                            in_str = Some(c);
                        } else if c as char == open {
                            depth += 1;
                        } else if c as char == close {
                            depth -= 1;
                        }
                        p += 1;
                    }
                    if depth != 0 {
                        break;
                    }
                    end = p;
                }
                b'.' => {
                    let mut p = end + 1;
                    if p >= bytes.len() || !(bytes[p].is_ascii_alphabetic() || bytes[p] == b'_') {
                        break;
                    }
                    p += 1;
                    while p < bytes.len()
                        && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_')
                    {
                        p += 1;
                    }
                    end = p;
                }
                _ => break,
            }
        }
        let operand = &after[..end];
        out = format!("{}{}.length{}", &out[..hash_pos], operand, &after[end..]);
    }
    out
}

fn split_strings(s: &str) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut buf = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            if !buf.is_empty() {
                out.push((std::mem::take(&mut buf), false));
            }
            let quote = c;
            let mut s_buf = String::new();
            s_buf.push(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                s_buf.push(ch);
                i += 1;
                if ch == '\\' && i < chars.len() {
                    s_buf.push(chars[i]);
                    i += 1;
                    continue;
                }
                if ch == quote {
                    break;
                }
            }
            out.push((s_buf, true));
            continue;
        }
        buf.push(c);
        i += 1;
    }
    if !buf.is_empty() {
        out.push((buf, false));
    }
    out
}

fn convert_code_segment(s: &str) -> String {
    let mut out = s.to_string();

    // Lua method call: obj:method(...) → obj.method(...)
    {
        let chars: Vec<char> = out.chars().collect();
        let mut result = String::with_capacity(out.len());
        let mut i = 0;
        while i < chars.len() {
            if chars[i] == ':' {
                let prev = if i > 0 { chars[i - 1] } else { ' ' };
                let next = if i + 1 < chars.len() { chars[i + 1] } else { ' ' };
                let prev_is_ident =
                    prev.is_alphanumeric() || prev == '_' || prev == ')' || prev == ']';
                let next_is_ident = next.is_alphabetic() || next == '_';
                if prev_is_ident && next_is_ident {
                    result.push('.');
                    i += 1;
                    continue;
                }
            }
            result.push(chars[i]);
            i += 1;
        }
        out = result;
    }

    // nil → null
    out = regex_replace_word(&out, "nil", "null");

    // Phase D — `super.x` (Lua extends) → `__parent__.x`. JavaScript reserves
    // `super` (only valid inside class methods); rewriting to `__parent__`
    // matches the identifier the host JS extends wrapper installs around the
    // child module's IIFE.
    out = regex_replace_word(&out, "super", "__parent__");

    // ~= → !==
    out = out.replace("~=", "!==");

    // .. → + (string concat)
    {
        let chars: Vec<char> = out.chars().collect();
        let mut result = String::with_capacity(out.len());
        let mut i = 0;
        while i < chars.len() {
            if i + 1 < chars.len() && chars[i] == '.' && chars[i + 1] == '.' {
                let prev = if i > 0 { chars[i - 1] } else { ' ' };
                let next = if i + 2 < chars.len() { chars[i + 2] } else { ' ' };
                if prev != '.' && next != '.' {
                    result.push_str(" + ");
                    i += 2;
                    continue;
                }
            }
            result.push(chars[i]);
            i += 1;
        }
        out = result;
    }

    // Boolean operators
    out = regex_replace_word(&out, "and", "&&");
    out = regex_replace_word(&out, "or", "||");
    out = regex_replace_word(&out, "not", "!");
    out = regex_replace_word(&out, "true", "true");
    out = regex_replace_word(&out, "false", "false");

    // math.* → Math.*
    out = out.replace("math.pi", "Math.PI");
    out = out.replace("math.huge", "Infinity");
    out = out.replace("math.floor", "Math.floor");
    out = out.replace("math.ceil", "Math.ceil");
    out = out.replace("math.abs", "Math.abs");
    out = out.replace("math.sqrt", "Math.sqrt");
    out = out.replace("math.sin", "Math.sin");
    out = out.replace("math.cos", "Math.cos");
    out = out.replace("math.atan", "Math.atan2");
    out = out.replace("math.random", "Math.random");
    out = out.replace("math.max", "Math.max");
    out = out.replace("math.min", "Math.min");

    out = out.replace("os.clock()", "(performance.now()/1000)");
    out = out.replace("tostring(", "String(");

    // table.insert / table.remove
    while out.contains("table.insert(") {
        if let Some(start) = out.find("table.insert(") {
            if let Some(close) = find_matching_paren(&out, start + 12) {
                let inner = &out[start + 13..close];
                if let Some(comma) = find_top_level_comma(inner) {
                    let tbl = inner[..comma].trim().to_string();
                    let val = inner[comma + 1..].trim().to_string();
                    out = format!(
                        "{}{}.push({}){}",
                        &out[..start],
                        tbl,
                        val,
                        &out[close + 1..]
                    );
                    continue;
                }
            }
        }
        break;
    }
    while out.contains("table.remove(") {
        if let Some(start) = out.find("table.remove(") {
            if let Some(close) = find_matching_paren(&out, start + 12) {
                let inner = &out[start + 13..close];
                if let Some(comma) = find_top_level_comma(inner) {
                    let tbl = inner[..comma].trim().to_string();
                    let idx = inner[comma + 1..].trim().to_string();
                    out = format!(
                        "{}{}.splice({}-1, 1){}",
                        &out[..start],
                        tbl,
                        idx,
                        &out[close + 1..]
                    );
                    continue;
                } else {
                    let tbl = inner.trim().to_string();
                    out = format!("{}{}.pop(){}", &out[..start], tbl, &out[close + 1..]);
                    continue;
                }
            }
        }
        break;
    }

    out
}

fn convert_lua_tables(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let (out, _) = convert_tables_range(&chars, 0, false);
    out
}

fn convert_tables_range(chars: &[char], start: usize, inside_braces: bool) -> (String, usize) {
    let mut out = String::new();
    let mut i = start;
    while i < chars.len() {
        let c = chars[i];

        if c == '"' || c == '\'' {
            let quote = c;
            out.push(c);
            i += 1;
            while i < chars.len() {
                let ch = chars[i];
                out.push(ch);
                i += 1;
                if ch == '\\' && i < chars.len() {
                    out.push(chars[i]);
                    i += 1;
                    continue;
                }
                if ch == quote {
                    break;
                }
            }
            continue;
        }

        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            while i < chars.len() && chars[i] != '\n' {
                out.push(chars[i]);
                i += 1;
            }
            continue;
        }

        if c == '{' {
            let (inner, end) = convert_tables_range(chars, i + 1, true);
            let converted = convert_table_body(&inner);
            out.push_str(&converted);
            i = if end < chars.len() { end + 1 } else { end };
            continue;
        }

        if c == '}' && inside_braces {
            return (out, i);
        }

        out.push(c);
        i += 1;
    }
    (out, i)
}

fn convert_table_body(inner: &str) -> String {
    if !scan_has_record_eq(inner) {
        return format!("[{}]", inner);
    }

    let chars: Vec<char> = inner.chars().collect();
    let mut out = String::with_capacity(inner.len() + 4);
    out.push('{');

    let mut depth_paren: i32 = 0;
    let mut depth_brack: i32 = 0;
    let mut depth_brace: i32 = 0;
    let mut seg_start = 0usize;
    let mut i = 0usize;

    let flush = |out: &mut String, chars: &[char], seg_start: usize, i: usize| {
        let seg: String = chars[seg_start..i].iter().collect();
        out.push_str(&convert_field_eq(&seg));
    };

    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let q = c;
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == q {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        match c {
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_brack += 1,
            ']' => depth_brack -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            _ => {}
        }
        if (c == ',' || c == ';')
            && depth_paren == 0
            && depth_brack == 0
            && depth_brace == 0
        {
            flush(&mut out, &chars, seg_start, i);
            out.push(c);
            seg_start = i + 1;
        }
        i += 1;
    }
    if seg_start < chars.len() {
        flush(&mut out, &chars, seg_start, chars.len());
    }

    out.push('}');
    out
}

fn scan_has_record_eq(inner: &str) -> bool {
    let chars: Vec<char> = inner.chars().collect();
    let mut depth_paren: i32 = 0;
    let mut depth_brack: i32 = 0;
    let mut depth_brace: i32 = 0;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' || c == '\'' {
            let q = c;
            i += 1;
            while i < chars.len() {
                if chars[i] == '\\' {
                    i += 2;
                    continue;
                }
                if chars[i] == q {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '-' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        match c {
            '(' => depth_paren += 1,
            ')' => depth_paren -= 1,
            '[' => depth_brack += 1,
            ']' => depth_brack -= 1,
            '{' => depth_brace += 1,
            '}' => depth_brace -= 1,
            _ => {}
        }
        if c == '=' && depth_paren == 0 && depth_brack == 0 && depth_brace == 0 {
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = if i + 1 < chars.len() { chars[i + 1] } else { ' ' };
            if prev != '=' && prev != '!' && prev != '<' && prev != '>' && prev != '~' && next != '='
            {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn convert_field_eq(seg: &str) -> String {
    let chars: Vec<char> = seg.chars().collect();
    let mut i = 0usize;
    // Skip whitespace *and* any `--` comments carried over from the previous
    // field's line; the key we are looking for comes after them.
    loop {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i + 1 < chars.len() && chars[i] == '-' && chars[i + 1] == '-' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        break;
    }
    if i >= chars.len() {
        return seg.to_string();
    }
    if chars[i] == '[' {
        let mut depth = 1;
        i += 1;
        while i < chars.len() && depth > 0 {
            if chars[i] == '[' {
                depth += 1;
            } else if chars[i] == ']' {
                depth -= 1;
            }
            i += 1;
        }
    } else if chars[i].is_alphabetic() || chars[i] == '_' {
        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
            i += 1;
        }
    } else {
        return seg.to_string();
    }
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    if i >= chars.len() || chars[i] != '=' {
        return seg.to_string();
    }
    if i + 1 < chars.len() && chars[i + 1] == '=' {
        return seg.to_string();
    }
    let mut out = String::with_capacity(seg.len());
    out.extend(chars[..i].iter());
    out.push(':');
    out.extend(chars[i + 1..].iter());
    out
}

fn regex_replace_word(s: &str, word: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut remaining = s;
    while let Some(pos) = remaining.find(word) {
        let before_ok = pos == 0
            || !remaining.as_bytes()[pos - 1].is_ascii_alphanumeric()
                && remaining.as_bytes()[pos - 1] != b'_';
        let end = pos + word.len();
        let after_ok = end >= remaining.len()
            || !remaining.as_bytes()[end].is_ascii_alphanumeric()
                && remaining.as_bytes()[end] != b'_';
        result.push_str(&remaining[..pos]);
        if before_ok && after_ok {
            result.push_str(replacement);
        } else {
            result.push_str(word);
        }
        remaining = &remaining[end..];
    }
    result.push_str(remaining);
    result
}

fn find_matching_paren(s: &str, open_pos: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.get(open_pos) != Some(&b'(') {
        return None;
    }
    let mut depth = 1;
    for i in (open_pos + 1)..bytes.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_top_level_comma(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, b) in s.bytes().enumerate() {
        match b {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth -= 1,
            b',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn split_top_level_commas(s: &str) -> Vec<&str> {
    let mut depth = 0i32;
    let mut start = 0usize;
    let mut out = Vec::new();
    let bytes = s.as_bytes();

    for (i, b) in bytes.iter().enumerate() {
        match *b {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b',' if depth == 0 => {
                out.push(&s[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }

    out.push(&s[start..]);
    out
}

/// Find the byte index of a top-level `=` that is a plain assignment
/// (not part of `==`, `~=`, `<=`, `>=`, `!=`). Returns `None` when no
/// such assignment exists at depth 0. Skips characters inside string
/// literals so values like `"a=b"` don't trigger false matches.
fn find_top_level_assign(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_str: Option<u8> = None;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if let Some(q) = in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' => in_str = Some(c),
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b'=' if depth == 0 => {
                let prev = if i > 0 { bytes[i - 1] } else { b' ' };
                let next = if i + 1 < bytes.len() { bytes[i + 1] } else { b' ' };
                // Reject `==`, `~=`, `<=`, `>=`, `!=` and the start of `=>`.
                if prev != b'=' && prev != b'~' && prev != b'<' && prev != b'>'
                    && prev != b'!'
                    && next != b'='
                    && next != b'>'
                {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// True when `s` is an identifier or a chain of `ident.field` /
/// `ident[expr]` accesses — i.e. something safe to put on the LHS of a
/// JS array-destructuring assignment. Rejects function calls and
/// arbitrary expressions.
fn is_simple_lvalue(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() {
        return false;
    }
    let bytes = s.as_bytes();
    // Must start with letter or underscore.
    if !(bytes[0].is_ascii_alphabetic() || bytes[0] == b'_') {
        return false;
    }
    let mut depth = 0i32;
    for &b in bytes {
        match b {
            b'[' => depth += 1,
            b']' => depth -= 1,
            b'(' | b')' => return false, // calls disqualify
            _ if depth > 0 => {}
            _ => {
                if !(b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'[' || b == b']') {
                    return false;
                }
            }
        }
    }
    depth == 0
}

struct IpairsCapture {
    idx_var: String,
    val_var: String,
    table: String,
}

fn parse_ipairs_for(body: &str) -> Option<IpairsCapture> {
    let in_pos = body.find(" in ")?;
    let vars = body[..in_pos].trim();
    let iter_expr = body[in_pos + 4..].trim();
    let var_parts: Vec<&str> = vars.split(',').map(|s| s.trim()).collect();
    if var_parts.len() != 2 {
        return None;
    }
    let idx_var = var_parts[0].to_string();
    let val_var = var_parts[1].to_string();
    let inner = iter_expr.strip_prefix("ipairs(")?.strip_suffix(')')?;
    Some(IpairsCapture {
        idx_var,
        val_var,
        table: inner.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_to_var() {
        let out = transpile_lua_to_js("local x = 1");
        assert!(out.contains("var x = 1"), "got: {out}");
    }

    #[test]
    fn function_block() {
        let out = transpile_lua_to_js("function update(dt)\n  local v = 1\nend");
        assert!(out.contains("function update(dt) {"), "got: {out}");
        assert!(out.contains("var v = 1"), "got: {out}");
        assert!(out.trim_end().ends_with('}'), "got: {out}");
    }

    #[test]
    fn ne_and_concat() {
        let out = transpile_lua_to_js("if a ~= b then x = a .. b end");
        assert!(out.contains("!=="), "got: {out}");
        assert!(out.contains(" + "), "got: {out}");
    }

    #[test]
    fn ipairs_loop() {
        let out = transpile_lua_to_js("for _, v in ipairs(items) do\n  print(v)\nend");
        assert!(out.contains("items.length"), "got: {out}");
        assert!(out.contains("var v ="), "got: {out}");
    }

    #[test]
    fn record_table_literal() {
        // Whitespace around the original `=` is preserved (e.g. "x = 1" -> "x : 1").
        let out = transpile_lua_to_js("local t = { x = 1, y = 2 }");
        assert!(out.contains("x : 1"), "got: {out}");
        assert!(out.contains("y : 2"), "got: {out}");
        assert!(out.contains('{') && out.contains('}'), "got: {out}");
    }

    #[test]
    fn array_table_literal() {
        let out = transpile_lua_to_js("local t = {1, 2, 3}");
        assert!(out.contains("[1, 2, 3]"), "got: {out}");
    }

    #[test]
    fn return_multi_value_becomes_array() {
        let out = transpile_lua_to_js("function axis()\n  return 1, 2\nend");
        assert!(out.contains("return [1, 2]"), "got: {out}");
    }

    #[test]
    fn local_destructure_from_return_multi_value() {
        let out = transpile_lua_to_js(
            "function axis()\n  local x = 1\n  local y = 2\n  return x, y\nend\nlocal a, b = axis()",
        );
        assert!(out.contains("return [x, y]"), "got: {out}");
        assert!(out.contains("var [a, b] = axis()"), "got: {out}");
    }

    #[test]
    fn local_multi_assign_from_expr_list_wraps_array() {
        // Without wrapping the RHS, JS would parse `var [dx, dy] = ax - bx, ay - by`
        // as two declarators and choke on the bare `-`.
        let out = transpile_lua_to_js("local dx, dy = ax - bx, ay - by");
        assert!(
            out.contains("var [dx, dy] = [ax - bx, ay - by]"),
            "got: {out}"
        );
    }

    #[test]
    fn hash_on_call_expression() {
        let out = transpile_lua_to_js("local n = #find_by_tag(\"enemy\")");
        assert!(
            out.contains("find_by_tag(\"enemy\").length"),
            "got: {out}"
        );
        assert!(!out.contains("find_by_tag.length"), "got: {out}");
    }

    #[test]
    fn hash_on_index_and_field_chain() {
        let out = transpile_lua_to_js("local n = #self.items[0].kids");
        assert!(out.contains("self.items[0].kids.length"), "got: {out}");
    }

    // ---- Phase D additions -------------------------------------------------

    #[test]
    fn super_keyword_rewrites_to_parent() {
        let out = transpile_lua_to_js("function attack(e)\n  super.attack(e)\nend");
        assert!(out.contains("__parent__.attack(e)"), "got: {out}");
        assert!(!out.contains("super."), "should not contain bare super.: {out}");
    }

    #[test]
    fn super_does_not_rewrite_inside_strings() {
        let out = transpile_lua_to_js("local s = \"super.foo\"");
        assert!(out.contains("\"super.foo\""), "got: {out}");
    }

    #[test]
    fn module_export_assignment_single_line() {
        let out = transpile_lua_to_js("local M = {}\nM.add = function(a, b) return a + b end\nreturn M");
        assert!(out.contains("M.add = function(a, b) {"), "got: {out}");
        assert!(out.contains("return a + b"), "got: {out}");
    }

    #[test]
    fn module_export_assignment_multi_line() {
        let out = transpile_lua_to_js(
            "local M = {}\nM.greet = function(name)\n  return \"hi \" + name\nend\nreturn M",
        );
        assert!(out.contains("M.greet = function(name) {"), "got: {out}");
        // The multi-line `end` is closed by the existing handler.
        assert!(out.contains("}"), "got: {out}");
    }

    // ---------------------------------------------------------------
    // Regressions from getting a game's web build to run. Each of
    // these shapes made a whole script fail to compile in the browser
    // while working fine on native Luau and in the Fengari editor
    // runtime, so they were invisible until a deployed build was tried.
    // ---------------------------------------------------------------

    #[test]
    fn ipairs_over_expression_is_parenthesised() {
        let out = transpile_lua_to_js("for _, v in ipairs(get_list() or {}) do\n  f(v)\nend");
        assert!(out.contains("(get_list() || [])"), "got: {out}");
        assert!(!out.contains(" or "), "operator left untranslated: {out}");
    }

    #[test]
    fn inline_break_after_statement_gets_separator() {
        let out = transpile_lua_to_js("if a == b then i = 1 break end");
        assert!(out.contains("i = 1; break"), "got: {out}");
    }

    #[test]
    fn multi_value_return_keeps_comment_outside_array() {
        let out = transpile_lua_to_js("return 1.0, 0.0 -- note");
        assert!(out.contains("return [1.0, 0.0]"), "got: {out}");
        assert!(!out.contains("0.0 // note]"), "comment swallowed `]`: {out}");
    }

    #[test]
    fn module_table_function_becomes_assignment() {
        let out = transpile_lua_to_js("function M.foo(a)\n  return a\nend");
        assert!(out.contains("M.foo = function(a) {"), "got: {out}");
    }

    #[test]
    fn colon_method_gains_self_parameter() {
        let out = transpile_lua_to_js("function M:bar(a)\n  return a\nend");
        assert!(out.contains("M.bar = function(self, a) {"), "got: {out}");
    }

    #[test]
    fn reserved_word_identifier_is_renamed() {
        let out = transpile_lua_to_js("local function line(new, old)\n  return new\nend");
        assert!(out.contains("new_"), "got: {out}");
        assert!(!out.contains("(new,"), "reserved word left as a parameter: {out}");
    }

    #[test]
    fn reserved_word_property_is_left_alone() {
        let out = transpile_lua_to_js("local v = row.new");
        assert!(out.contains("row.new"), "property renamed: {out}");
    }

    #[test]
    fn single_line_loop_with_body() {
        let out = transpile_lua_to_js("for i = 0, 3 do t[i] = nil end");
        assert!(out.contains("for (var i = 0; i <= 3; i++) {"), "got: {out}");
        assert!(out.contains("t[i] = null }"), "got: {out}");
    }

    #[test]
    fn pairs_loop_binds_key_and_value() {
        let out = transpile_lua_to_js("for k, v in pairs(counts) do\n  f(k, v)\nend");
        assert!(out.contains("for (var k in counts) {"), "got: {out}");
        assert!(out.contains("var v = counts[k];"), "got: {out}");
    }

    #[test]
    fn gmatch_loop_becomes_regex_iteration() {
        let out = transpile_lua_to_js("for name in string.gmatch(packed, \"[^|]+\") do\n  f(name)\nend");
        assert!(out.contains("String(packed).match(new RegExp("), "got: {out}");
        assert!(out.contains("\"g\""), "missing global flag: {out}");
    }

    #[test]
    fn single_line_if_elseif_chain() {
        let out = transpile_lua_to_js("if p < 0 then p = 0 elseif p > 1 then p = 1 end");
        assert!(out.contains("if (p < 0) { p = 0 } else if (p > 1) { p = 1 }"), "got: {out}");
    }

    #[test]
    fn semicolon_separated_statements_are_split() {
        let out = transpile_lua_to_js("local c = f(); if c ~= 0 then g(c) end");
        assert!(out.contains("var c = f()"), "got: {out}");
        assert!(out.contains("if (c !== 0) { g(c) }"), "got: {out}");
    }

    #[test]
    fn trailing_comment_does_not_block_end_anchored_rules() {
        let out = transpile_lua_to_js("if x == nil then return end -- why");
        // `== nil` becomes `== null`, which in JS also covers `undefined` -
        // the right reading of a Lua nil check.
        assert!(out.contains("if (x == null) { return }"), "got: {out}");
    }

    #[test]
    fn wrapped_condition_is_folded_onto_one_line() {
        let out = transpile_lua_to_js("if a()\n    or b() then\n  c()\nend");
        assert!(out.contains("if (a() || b()) {"), "got: {out}");
    }

    #[test]
    fn record_table_fields_after_comments_still_convert() {
        let out = transpile_lua_to_js(
            "local T = {\n  a = { 1 }, -- first\n  b = { 2 }, -- second\n}",
        );
        assert!(out.contains("a : [ 1 ]"), "got: {out}");
        assert!(out.contains("b : [ 2 ]"), "second field not converted: {out}");
    }

    #[test]
    fn inline_body_local_declaration_becomes_var() {
        let out = transpile_lua_to_js("if e ~= 0 then local x, y = f(); g(e, x, y) end");
        assert!(out.contains("var [x, y] = f()"), "got: {out}");
    }

    #[test]
    fn single_line_local_function_with_body() {
        let out = transpile_lua_to_js("local function line(s) L[#L + 1] = s end");
        assert!(out.contains("function line(s) { L[L.length + 1] = s }"), "got: {out}");
    }
}
