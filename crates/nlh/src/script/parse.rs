//! Top-level structure: `param` / `define` declarations, token-level
//! substitution, and the recursive-descent parser for street blocks and
//! their bodies. [`Script::compile`] is the single entry point that ties
//! tokenizing, substitution, parsing, and lowering together.
//!
//! Pipeline, matching the module's flow diagram in the spec:
//!
//! 1. [`super::token::tokenize`] the whole source once.
//! 2. Harvest `param` descriptions from the comment lines directly above
//!    each declaration, before comments are discarded.
//! 3. [`scan_top_level`] walks the (comment-free) token stream once, at
//!    brace depth zero, splitting it into `param` / `define` declarations
//!    and street-block token spans (street list, optional shorthand `when`
//!    tokens, and body tokens, each still raw/unsubstituted).
//! 4. [`resolve_declarations`] processes the declarations in source order,
//!    substituting each one's own references to earlier params/defines as
//!    it goes (catching self- and forward-references), and builds the
//!    `param` schema.
//! 5. Each street block's `when`/body tokens are substituted once against
//!    the now-complete table (`substitute_tokens`) -- *before* anything
//!    parses them as a condition or a size list, per the spec -- then
//!    parsed by the recursive-descent block/condition/size grammar.
//! 6. [`super::lower::lower`] flattens the parsed blocks into the compiled
//!    `Script`'s rule list.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::ScriptError;
use super::ast::{ActionKind, Effect, ParamKind, ParamSchema, Script, StmtAst, StreetBlockAst};
use super::cond::{self, Dialect, Vars, is_reserved};
use super::lower;
use super::token::{self, Token, TokenKind, expect_punct, is_punct, is_word, line_at, tokenize};
use crate::{SizeSpec, Street};

impl<V: Vars> Script<V> {
    /// Compiles a `.tree` script source against `dialect`, applying
    /// `overrides` to any `param` they name. `overrides` keys that do not
    /// name a declared `param` are an error, as is an override, `param`, or
    /// `define` value that does not resolve to a single token.
    pub fn compile(
        source: &str,
        overrides: &BTreeMap<String, String>,
        dialect: &Dialect<V>,
    ) -> Result<Script<V>, ScriptError> {
        let all_tokens = tokenize(source)?;
        let line_info = build_line_info(&all_tokens);
        let significant: Vec<Token> = all_tokens
            .into_iter()
            .filter(|t| !matches!(t.kind, TokenKind::Comment(_)))
            .collect();

        let items = scan_top_level(&significant, dialect)?;
        let (resolved, params) = resolve_declarations(&items, &line_info, overrides, dialect)?;

        let mut blocks = Vec::with_capacity(items.len());
        for item in &items {
            if let TopLevel::Street(street_item) = item {
                blocks.push(parse_street_block(street_item, &resolved, dialect)?);
            }
        }

        Ok(Script {
            params,
            rules: lower::lower(blocks),
        })
    }
}

// ---- param description harvesting ----------------------------------------

struct LineInfo {
    has_code: bool,
    comment: Option<String>,
}

/// Builds a per-line summary of `# comment` text and whether the line also
/// carries any non-comment token, from the *full* token stream (comments
/// included). Used to find the run of comment-only lines directly above a
/// `param`.
fn build_line_info(tokens: &[Token]) -> HashMap<usize, LineInfo> {
    let mut map: HashMap<usize, LineInfo> = HashMap::new();
    for token in tokens {
        let entry = map.entry(token.line).or_insert(LineInfo {
            has_code: false,
            comment: None,
        });
        match &token.kind {
            TokenKind::Comment(text) => {
                let stripped = text.strip_prefix('#').unwrap_or(text);
                let stripped = stripped.strip_prefix(' ').unwrap_or(stripped);
                entry.comment = Some(stripped.to_string());
            }
            _ => entry.has_code = true,
        }
    }
    map
}

/// The run of comment-only lines directly above `declaration_line`, oldest
/// first and joined with `\n` -- a `param`'s description. A trailing
/// comment on a code line does not count, since that line's `has_code` is
/// `true`.
fn description_above(
    line_info: &HashMap<usize, LineInfo>,
    declaration_line: usize,
) -> Option<String> {
    let mut lines = Vec::new();
    let mut line = declaration_line;
    while line > 1 {
        line -= 1;
        match line_info.get(&line) {
            Some(info) if !info.has_code && info.comment.is_some() => {
                lines.push(info.comment.clone().expect("checked is_some above"));
            }
            _ => break,
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

// ---- top-level scan --------------------------------------------------------

struct StreetItem {
    streets: Vec<(String, usize)>,
    when_tokens: Vec<Token>,
    body_tokens: Vec<Token>,
    line: usize,
}

enum TopLevel {
    Param {
        name: String,
        name_line: usize,
        value_token: Token,
    },
    Define {
        name: String,
        name_line: usize,
        tokens: Vec<Token>,
    },
    Street(StreetItem),
}

/// True when `token` can legally start a new top-level item, i.e. is a
/// valid stopping point for a `param`'s single-token value.
fn starts_top_level<V: Vars>(token: &Token, dialect: &Dialect<V>) -> bool {
    match &token.kind {
        TokenKind::Word(word) => {
            word == "param" || word == "define" || dialect.streets.iter().any(|(n, _)| n == word)
        }
        _ => false,
    }
}

/// Walks the (comment-free) token stream once at brace depth zero, cutting
/// it into `param` / `define` declarations and street-block spans. Nested
/// content inside `{ ... }` is collected as opaque, not-yet-substituted
/// token spans; the block/condition grammar only sees it after
/// substitution (`parse_street_block`).
fn scan_top_level<V: Vars>(
    tokens: &[Token],
    dialect: &Dialect<V>,
) -> Result<Vec<TopLevel>, ScriptError> {
    let mut items = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        match &tokens[i].kind {
            TokenKind::Word(word) if word == "param" => {
                let line = tokens[i].line;
                i += 1;
                let (name, _) = expect_top_level_word(tokens, &mut i, "a param name")?;
                expect_punct(tokens, &mut i, "=")?;
                let value_token = tokens.get(i).cloned().ok_or_else(|| ScriptError {
                    line,
                    message: format!("param {name:?} is missing a value"),
                })?;
                i += 1;
                if let Some(next) = tokens.get(i)
                    && !starts_top_level(next, dialect)
                {
                    return Err(ScriptError {
                        line: next.line,
                        message: format!(
                            "param {name:?} value must be a single token: writing e.g. \
                             `param wet = a || b` would silently expand `wet && c` into \
                             `a || b && c` and change its precedence"
                        ),
                    });
                }
                items.push(TopLevel::Param {
                    name,
                    name_line: line,
                    value_token,
                });
            }
            TokenKind::Word(word) if word == "define" => {
                let line = tokens[i].line;
                i += 1;
                let (name, _) = expect_top_level_word(tokens, &mut i, "a define name")?;
                expect_punct(tokens, &mut i, "=")?;
                let mut condition_tokens = Vec::new();
                while let Some(token) = tokens.get(i) {
                    if token.line != line {
                        break;
                    }
                    condition_tokens.push(token.clone());
                    i += 1;
                }
                if condition_tokens.is_empty() {
                    return Err(ScriptError {
                        line,
                        message: format!("define {name:?} has no condition"),
                    });
                }
                items.push(TopLevel::Define {
                    name,
                    name_line: line,
                    tokens: condition_tokens,
                });
            }
            TokenKind::Word(_) => {
                let start_line = tokens[i].line;
                let mut streets = Vec::new();
                loop {
                    let (name, line) = expect_top_level_word(tokens, &mut i, "a street name")?;
                    streets.push((name, line));
                    if is_punct(tokens, i, ",") {
                        i += 1;
                        continue;
                    }
                    break;
                }
                let mut when_tokens = Vec::new();
                if is_word(tokens, i, "when") {
                    i += 1;
                    loop {
                        let token = tokens.get(i).ok_or_else(|| ScriptError {
                            line: start_line,
                            message: "expected '{' after 'when' condition".to_string(),
                        })?;
                        if is_punct(tokens, i, "{") {
                            break;
                        }
                        when_tokens.push(token.clone());
                        i += 1;
                    }
                }
                expect_punct(tokens, &mut i, "{")?;
                let mut depth = 1i32;
                let mut body_tokens = Vec::new();
                loop {
                    let token = tokens.get(i).ok_or_else(|| ScriptError {
                        line: start_line,
                        message: "unterminated block: missing '}'".to_string(),
                    })?;
                    match &token.kind {
                        TokenKind::Punct("{") => {
                            depth += 1;
                            body_tokens.push(token.clone());
                        }
                        TokenKind::Punct("}") => {
                            depth -= 1;
                            if depth == 0 {
                                i += 1;
                                break;
                            }
                            body_tokens.push(token.clone());
                        }
                        _ => body_tokens.push(token.clone()),
                    }
                    i += 1;
                }
                items.push(TopLevel::Street(StreetItem {
                    streets,
                    when_tokens,
                    body_tokens,
                    line: start_line,
                }));
            }
            other => {
                return Err(ScriptError {
                    line: tokens[i].line,
                    message: format!("unexpected token {other:?} at top level"),
                });
            }
        }
    }
    Ok(items)
}

fn expect_top_level_word(
    tokens: &[Token],
    pos: &mut usize,
    what: &str,
) -> Result<(String, usize), ScriptError> {
    match tokens.get(*pos) {
        Some(Token {
            kind: TokenKind::Word(word),
            line,
        }) => {
            let result = (word.clone(), *line);
            *pos += 1;
            Ok(result)
        }
        Some(other) => Err(ScriptError {
            line: other.line,
            message: format!("expected {what}"),
        }),
        None => Err(ScriptError {
            line: line_at(tokens, *pos),
            message: format!("expected {what}"),
        }),
    }
}

// ---- declaration resolution and substitution ------------------------------

enum Resolution {
    Param(Token),
    Define(Vec<Token>),
}

/// Replaces every `Word` token that names a resolved `param` or `define`.
/// A `param` reference becomes its single value token; a `define`
/// reference is spliced in as `( <its tokens> )` -- the parentheses are
/// what make `define wet = a || b` behave correctly when substituted into
/// `wet && c`. Each `Resolution::Define` already holds *fully* resolved
/// tokens (its own references were substituted when it was declared), so
/// this is a single non-recursive pass.
fn substitute_tokens(tokens: &[Token], resolved: &HashMap<String, Resolution>) -> Vec<Token> {
    let mut out = Vec::with_capacity(tokens.len());
    for token in tokens {
        if let TokenKind::Word(word) = &token.kind
            && let Some(resolution) = resolved.get(word)
        {
            match resolution {
                Resolution::Param(value) => out.push(Token {
                    kind: value.kind.clone(),
                    line: token.line,
                }),
                Resolution::Define(define_tokens) => {
                    out.push(Token {
                        kind: TokenKind::Punct("("),
                        line: token.line,
                    });
                    out.extend(define_tokens.iter().cloned());
                    out.push(Token {
                        kind: TokenKind::Punct(")"),
                        line: token.line,
                    });
                }
            }
            continue;
        }
        out.push(token.clone());
    }
    out
}

/// Errors if `tokens` (a `param`'s raw value or a `define`'s raw condition)
/// references itself, or references a param/define declared later in the
/// file. Forward references are rejected outright rather than resolved
/// lazily, so the substitution table can be built with one linear pass and
/// never needs cycle detection.
fn check_self_and_forward_references(
    name: &str,
    tokens: &[Token],
    declared_names: &HashSet<String>,
    resolved: &HashMap<String, Resolution>,
) -> Result<(), ScriptError> {
    for token in tokens {
        if let TokenKind::Word(word) = &token.kind {
            if word == name {
                return Err(ScriptError {
                    line: token.line,
                    message: format!("{name:?} references itself"),
                });
            }
            if declared_names.contains(word) && !resolved.contains_key(word) {
                return Err(ScriptError {
                    line: token.line,
                    message: format!(
                        "{name:?} references {word:?}, which is declared later in the file"
                    ),
                });
            }
        }
    }
    Ok(())
}

fn render_token(kind: &TokenKind) -> String {
    match kind {
        TokenKind::Word(word) => word.clone(),
        TokenKind::Str(text) => format!("\"{text}\""),
        TokenKind::Punct(punct) => (*punct).to_string(),
        TokenKind::Comment(text) => text.clone(),
    }
}

fn infer_param_kind(value_text: &str) -> ParamKind {
    if value_text == "true" || value_text == "false" {
        ParamKind::Bool
    } else if value_text.parse::<f64>().is_ok() {
        ParamKind::Number
    } else {
        ParamKind::Token
    }
}

/// Processes every `param` / `define` in source order, building the
/// substitution table and the `param` schema. Reserved names and duplicate
/// declarations are rejected up front; each declaration's own references
/// are then checked and substituted using only what has already been
/// resolved, before it is itself marked resolved.
fn resolve_declarations<V: Vars>(
    items: &[TopLevel],
    line_info: &HashMap<usize, LineInfo>,
    overrides: &BTreeMap<String, String>,
    dialect: &Dialect<V>,
) -> Result<(HashMap<String, Resolution>, Vec<ParamSchema>), ScriptError> {
    let mut declared_names: HashSet<String> = HashSet::new();
    for item in items {
        let (name, name_line) = match item {
            TopLevel::Param {
                name, name_line, ..
            }
            | TopLevel::Define {
                name, name_line, ..
            } => (name, *name_line),
            TopLevel::Street(_) => continue,
        };
        if is_reserved(name, dialect) {
            return Err(ScriptError {
                line: name_line,
                message: format!(
                    "{name:?} is a reserved name and cannot be used as a param or define"
                ),
            });
        }
        if !declared_names.insert(name.clone()) {
            return Err(ScriptError {
                line: name_line,
                message: format!("{name:?} is already declared"),
            });
        }
    }

    let mut resolved: HashMap<String, Resolution> = HashMap::new();
    let mut params = Vec::new();
    let mut used_overrides: HashSet<String> = HashSet::new();

    for item in items {
        match item {
            TopLevel::Param {
                name,
                name_line,
                value_token,
            } => {
                let mut raw = vec![value_token.clone()];
                if let Some(override_value) = overrides.get(name) {
                    used_overrides.insert(name.clone());
                    let override_tokens: Vec<Token> = token::tokenize(override_value)
                        .map_err(|error| ScriptError {
                            line: *name_line,
                            message: format!(
                                "override for param {name:?} is not valid: {}",
                                error.message
                            ),
                        })?
                        .into_iter()
                        .filter(|t| !matches!(t.kind, TokenKind::Comment(_)))
                        .collect();
                    if override_tokens.len() != 1 {
                        return Err(ScriptError {
                            line: *name_line,
                            message: format!("override for param {name:?} must be a single token"),
                        });
                    }
                    raw = override_tokens;
                }
                check_self_and_forward_references(name, &raw, &declared_names, &resolved)?;
                let expanded = substitute_tokens(&raw, &resolved);
                if expanded.len() != 1 {
                    return Err(ScriptError {
                        line: *name_line,
                        message: format!(
                            "param {name:?} value must be a single token after substitution"
                        ),
                    });
                }
                let value_text = render_token(&expanded[0].kind);
                params.push(ParamSchema {
                    name: name.clone(),
                    kind: infer_param_kind(&value_text),
                    default: value_text,
                    description: description_above(line_info, *name_line),
                });
                resolved.insert(
                    name.clone(),
                    Resolution::Param(expanded.into_iter().next().expect("checked len == 1")),
                );
            }
            TopLevel::Define { name, tokens, .. } => {
                check_self_and_forward_references(name, tokens, &declared_names, &resolved)?;
                let expanded = substitute_tokens(tokens, &resolved);
                resolved.insert(name.clone(), Resolution::Define(expanded));
            }
            TopLevel::Street(_) => {}
        }
    }

    for key in overrides.keys() {
        if !used_overrides.contains(key) {
            return Err(ScriptError {
                line: 0,
                message: format!("override {key:?} does not name a declared param"),
            });
        }
    }

    Ok((resolved, params))
}

// ---- street block / body / condition / size-list parsing ------------------

fn parse_street_block<V: Vars>(
    item: &StreetItem,
    resolved: &HashMap<String, Resolution>,
    dialect: &Dialect<V>,
) -> Result<StreetBlockAst<V>, ScriptError> {
    let mut seen = HashSet::new();
    let mut streets = Vec::with_capacity(item.streets.len());
    for (name, line) in &item.streets {
        let street: Street = dialect
            .streets
            .iter()
            .find(|(street_name, _)| street_name == name)
            .map(|(_, street)| *street)
            .ok_or_else(|| ScriptError {
                line: *line,
                message: format!("unknown street {name:?}"),
            })?;
        if !seen.insert(name.clone()) {
            return Err(ScriptError {
                line: *line,
                message: format!("street {name:?} is repeated in the same street list"),
            });
        }
        streets.push(street);
    }

    let condition = if item.when_tokens.is_empty() {
        None
    } else {
        let substituted = substitute_tokens(&item.when_tokens, resolved);
        let mut pos = 0;
        let condition = cond::parse_condition(&substituted, &mut pos, dialect)?;
        if pos != substituted.len() {
            return Err(ScriptError {
                line: line_at(&substituted, pos),
                message: "unexpected token after 'when' condition".to_string(),
            });
        }
        Some(condition)
    };

    if item.body_tokens.is_empty() {
        return Err(ScriptError {
            line: item.line,
            message: "block body must not be empty".to_string(),
        });
    }
    let body_tokens = substitute_tokens(&item.body_tokens, resolved);
    let mut pos = 0;
    let body = parse_body(&body_tokens, &mut pos, dialect)?;
    if pos != body_tokens.len() {
        return Err(ScriptError {
            line: line_at(&body_tokens, pos),
            message: "unexpected token in block body".to_string(),
        });
    }

    Ok(StreetBlockAst {
        streets,
        condition,
        body,
    })
}

fn parse_body<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<Vec<StmtAst<V>>, ScriptError> {
    let mut stmts = Vec::new();
    while *pos < tokens.len() && !is_punct(tokens, *pos, "}") {
        stmts.push(parse_stmt(tokens, pos, dialect)?);
    }
    Ok(stmts)
}

fn parse_braced_body<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<Vec<StmtAst<V>>, ScriptError> {
    let open_line = line_at(tokens, *pos);
    expect_punct(tokens, pos, "{")?;
    if is_punct(tokens, *pos, "}") {
        return Err(ScriptError {
            line: open_line,
            message: "block body must not be empty".to_string(),
        });
    }
    let body = parse_body(tokens, pos, dialect)?;
    expect_punct(tokens, pos, "}")?;
    Ok(body)
}

/// The dialect's action word at `pos`, if any, without consuming it --
/// `dialect.actions` is what actually narrows the grammar per family
/// (postflop's lists only `bet`/`raise`; multiway's lists all five).
fn peek_action_word<V: Vars>(
    tokens: &[Token],
    pos: usize,
    dialect: &Dialect<V>,
) -> Option<ActionKind> {
    dialect
        .actions
        .iter()
        .copied()
        .find(|action| is_word(tokens, pos, action.name()))
}

fn parse_stmt<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<StmtAst<V>, ScriptError> {
    let line = line_at(tokens, *pos);

    if is_word(tokens, *pos, "when") {
        *pos += 1;
        let condition = cond::parse_condition(tokens, pos, dialect)?;
        let body = parse_braced_body(tokens, pos, dialect)?;
        return Ok(StmtAst::When { condition, body });
    }
    if is_word(tokens, *pos, "if") {
        return parse_if(tokens, pos, dialect);
    }
    if is_word(tokens, *pos, "checkdown") {
        *pos += 1;
        if peek_action_word(tokens, *pos, dialect).is_some() {
            return Err(ScriptError {
                line,
                message: "'checkdown' does not take an action; write 'checkdown' on its own"
                    .to_string(),
            });
        }
        if is_punct(tokens, *pos, "[") {
            return Err(ScriptError {
                line,
                message: "'checkdown' does not take a size list".to_string(),
            });
        }
        return Ok(StmtAst::Action {
            effect: Effect::Checkdown,
            action: None,
            sizes: Vec::new(),
        });
    }

    let effect = if is_word(tokens, *pos, "add") {
        Effect::Add
    } else if is_word(tokens, *pos, "remove") {
        Effect::Remove
    } else if is_word(tokens, *pos, "replace") {
        Effect::Replace
    } else if is_word(tokens, *pos, "force") {
        Effect::Force
    } else {
        return Err(ScriptError {
            line,
            message:
                "expected a statement ('add' / 'remove' / 'replace' / 'force' / 'checkdown'), \
                 'when', or 'if'"
                    .to_string(),
        });
    };
    *pos += 1;

    let action_line = line_at(tokens, *pos);
    let Some(action) = peek_action_word(tokens, *pos, dialect) else {
        let names = dialect
            .actions
            .iter()
            .map(|action| format!("'{}'", action.name()))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ScriptError {
            line: action_line,
            message: format!("expected one of: {names}"),
        });
    };
    *pos += 1;

    if effect == Effect::Remove {
        if is_punct(tokens, *pos, "[") {
            return Err(ScriptError {
                line: line_at(tokens, *pos),
                message: format!(
                    "'remove' does not take a size list; write 'remove {}' with no sizes",
                    action.name()
                ),
            });
        }
        return Ok(StmtAst::Action {
            effect,
            action: Some(action),
            sizes: Vec::new(),
        });
    }

    let sizes = parse_size_list(tokens, pos, dialect)?;
    Ok(StmtAst::Action {
        effect,
        action: Some(action),
        sizes,
    })
}

fn parse_if<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<StmtAst<V>, ScriptError> {
    *pos += 1; // consume 'if'
    let mut arms = Vec::new();
    let condition = cond::parse_condition(tokens, pos, dialect)?;
    let body = parse_braced_body(tokens, pos, dialect)?;
    arms.push((condition, body));

    let mut else_body = None;
    while is_word(tokens, *pos, "else") {
        *pos += 1;
        if is_word(tokens, *pos, "if") {
            *pos += 1;
            let condition = cond::parse_condition(tokens, pos, dialect)?;
            let body = parse_braced_body(tokens, pos, dialect)?;
            arms.push((condition, body));
            continue;
        }
        let body = parse_braced_body(tokens, pos, dialect)?;
        else_body = Some(body);
        break;
    }

    Ok(StmtAst::If { arms, else_body })
}

fn parse_size_list<V: Vars>(
    tokens: &[Token],
    pos: &mut usize,
    dialect: &Dialect<V>,
) -> Result<Vec<SizeSpec>, ScriptError> {
    expect_punct(tokens, pos, "[")?;
    let mut sizes = Vec::new();
    if !is_punct(tokens, *pos, "]") {
        loop {
            let token_line = line_at(tokens, *pos);
            let text = match tokens.get(*pos).map(|t| &t.kind) {
                Some(TokenKind::Word(word)) => word.clone(),
                _ => {
                    return Err(ScriptError {
                        line: token_line,
                        message: "expected a size literal".to_string(),
                    });
                }
            };
            *pos += 1;
            let spec = SizeSpec::parse(&text, dialect.unit).map_err(|error| ScriptError {
                line: token_line,
                message: error.to_string(),
            })?;
            sizes.push(spec);
            if is_punct(tokens, *pos, ",") {
                *pos += 1;
                continue;
            }
            break;
        }
    }
    expect_punct(tokens, pos, "]")?;
    Ok(sizes)
}

#[cfg(test)]
mod tests {
    use super::super::cond::{POSTFLOP, PostflopVar, PreviousAggressor, RuleContext};
    use super::*;
    use crate::BoardFacts;

    fn compile(source: &str) -> Result<Script<PostflopVar>, ScriptError> {
        Script::compile(source, &BTreeMap::new(), &POSTFLOP)
    }

    fn compile_with(
        source: &str,
        overrides: &[(&str, &str)],
    ) -> Result<Script<PostflopVar>, ScriptError> {
        let overrides = overrides
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Script::compile(source, &overrides, &POSTFLOP)
    }

    fn base_ctx() -> RuleContext {
        RuleContext {
            aggressions: 0,
            in_position: true,
            spr: 5.0,
            pot: 100.0,
            to_call: 0.0,
            previous_aggressor: PreviousAggressor::None,
            board: BoardFacts::new(&[
                "2h".parse().unwrap(),
                "7d".parse().unwrap(),
                "Kc".parse().unwrap(),
            ]),
        }
    }

    const SPEC_COMPLETE_TREE: &str = r#"
param cb     = 33
param barrel = 66
define wet   = flush_possible || straight_possible

flop {
  when donk {
    remove bet
  }
  when cbet {
    replace bet [cb]
    when wet { replace bet [cb, 75] }
  }
  when aggressions == 1 {
    replace raise [3x]
    when spr <= 3 { replace raise [a] }
  }
}

turn when unopened {
  if donk          { remove bet }
  else if spr <= 2 { force bet [a] }
  else             { replace bet [barrel] }
}

river {
  when unopened { replace bet [barrel, a] }
  when facing_pct >= 75 { remove raise }
}

turn, river when monotone {
  checkdown
}
"#;

    /// The spec's "`.tree` の完全な形" complete example must compile, with
    /// the exact rule count, streets, and effects it implies: 2 rules for
    /// the flop `donk`/`cbet` block plus 2 nested + 2 for `aggressions==1`
    /// (4 total on flop: donk, cbet, cbet&&wet, aggressions==1,
    /// aggressions==1&&spr<=3 -- 5 actually, see the assertion below), 3 for
    /// the `turn when unopened` if/else-if/else, 2 for river's own block,
    /// and 2 for the trailing `turn, river when monotone { checkdown }`
    /// (one per street).
    #[test]
    fn spec_complete_tree_compiles_with_the_expected_rule_shape() {
        let script = compile(SPEC_COMPLETE_TREE).unwrap();

        assert_eq!(script.params.len(), 2);
        assert_eq!(script.params[0].name, "cb");
        assert_eq!(script.params[0].default, "33");
        assert_eq!(script.params[0].kind, ParamKind::Number);
        assert_eq!(script.params[1].name, "barrel");
        assert_eq!(script.params[1].default, "66");

        // flop: donk, cbet, cbet&&wet, aggressions==1, aggressions==1&&spr<=3
        let flop_rules: Vec<_> = script
            .rules
            .iter()
            .filter(|r| r.street == Street::Flop)
            .collect();
        assert_eq!(flop_rules.len(), 5);

        // turn: unopened&&donk, unopened&&!donk&&spr<=2, unopened&&!donk&&!(spr<=2),
        // plus the trailing monotone checkdown block.
        let turn_rules: Vec<_> = script
            .rules
            .iter()
            .filter(|r| r.street == Street::Turn)
            .collect();
        assert_eq!(turn_rules.len(), 4);
        assert_eq!(turn_rules[3].effect, Effect::Checkdown);

        // river: unopened, facing_pct>=75, plus the trailing monotone checkdown.
        let river_rules: Vec<_> = script
            .rules
            .iter()
            .filter(|r| r.street == Street::River)
            .collect();
        assert_eq!(river_rules.len(), 3);
        assert_eq!(river_rules[2].effect, Effect::Checkdown);

        assert_eq!(script.rules.len(), 5 + 4 + 3);
    }

    #[test]
    fn param_override_replaces_the_effective_value() {
        let script = compile_with(SPEC_COMPLETE_TREE, &[("cb", "50")]).unwrap();
        assert_eq!(script.params[0].default, "50");
    }

    #[test]
    fn param_with_two_tokens_is_rejected() {
        let error = compile("param wet = a || b\nflop { checkdown }").unwrap_err();
        assert!(error.message.contains("single token"), "{}", error.message);
    }

    #[test]
    fn unknown_override_key_is_rejected() {
        let error = compile_with(SPEC_COMPLETE_TREE, &[("nope", "1")]).unwrap_err();
        assert!(error.message.contains("nope"));
    }

    #[test]
    fn reserved_name_param_is_rejected() {
        for reserved in ["paired", "a", "min", "true", "when", "flop", "bet"] {
            let source = format!("param {reserved} = 1\nflop {{ checkdown }}");
            let error = compile(&source).unwrap_err();
            assert!(
                error.message.contains(reserved),
                "{}: {}",
                reserved,
                error.message
            );
        }
    }

    #[test]
    fn reserved_name_define_is_rejected() {
        let error = compile("define paired = true\nflop { checkdown }").unwrap_err();
        assert!(error.message.contains("paired"));
    }

    #[test]
    fn self_reference_is_rejected() {
        let error = compile("define wet = wet || paired\nflop { checkdown }").unwrap_err();
        assert!(error.message.contains("itself"), "{}", error.message);
    }

    #[test]
    fn forward_reference_is_rejected() {
        let source = "define wet = dry || paired\ndefine dry = !paired\nflop { checkdown }";
        let error = compile(source).unwrap_err();
        assert!(
            error.message.contains("declared later"),
            "{}",
            error.message
        );
    }

    /// `define wet = paired || monotone` used as `wet && rainbow` must
    /// become `(paired || monotone) && rainbow`, not
    /// `paired || (monotone && rainbow)`.
    #[test]
    fn define_is_parenthesized_when_substituted() {
        let source = "define wet = paired || monotone\nflop when wet && rainbow { checkdown }";
        let script = compile(source).unwrap();
        assert_eq!(script.rules.len(), 1);
        let condition = &script.rules[0].condition;

        // paired, not monotone, not rainbow: `(paired || monotone) && rainbow`
        // is false (not rainbow); the wrong parse `paired || (monotone &&
        // rainbow)` would be true. This board is paired and two-tone (not
        // rainbow, not monotone).
        let mut ctx = base_ctx();
        ctx.board = BoardFacts::new(&[
            "9h".parse().unwrap(),
            "9d".parse().unwrap(),
            "2h".parse().unwrap(),
        ]);
        assert!(
            !condition.eval(&ctx),
            "wrong parse would wrongly match a paired non-rainbow board"
        );

        // paired, rainbow: both parses agree true, not discriminating.
        // monotone, rainbow is impossible, so instead check a genuinely
        // rainbow non-paired-non-monotone board is false under the correct
        // parse (since (paired||monotone) is false) but the wrong parse
        // `paired || (monotone && rainbow)` is also false here -- so use
        // the paired-and-two-tone board above as the discriminator, which
        // already distinguishes the two parses.
        let mut rainbow_ctx = base_ctx();
        rainbow_ctx.board = BoardFacts::new(&[
            "9h".parse().unwrap(),
            "9d".parse().unwrap(),
            "2s".parse().unwrap(),
        ]);
        assert!(
            condition.eval(&rainbow_ctx),
            "paired and rainbow should match under either parse"
        );
    }

    #[test]
    fn duplicate_street_in_one_list_is_rejected() {
        let error = compile("flop, flop { checkdown }").unwrap_err();
        assert!(error.message.contains("repeated"), "{}", error.message);
    }

    #[test]
    fn same_street_in_two_blocks_is_accepted() {
        let source = "flop { checkdown }\nflop when paired { checkdown }";
        let script = compile(source).unwrap();
        assert_eq!(script.rules.len(), 2);
    }

    #[test]
    fn empty_body_is_rejected_at_every_level() {
        assert!(compile("flop { }").is_err());
        assert!(compile("flop { when paired { } }").is_err());
        assert!(compile("flop { if paired { } }").is_err());
    }

    #[test]
    fn checkdown_with_action_or_sizes_is_rejected() {
        assert!(compile("flop { checkdown bet }").is_err());
        assert!(compile("flop { checkdown [33] }").is_err());
    }

    #[test]
    fn remove_with_sizes_is_rejected() {
        let error = compile("flop { remove bet [33] }").unwrap_err();
        assert!(error.message.contains("does not take a size list"));
    }

    #[test]
    fn add_with_empty_sizes_is_accepted() {
        let script = compile("flop { add bet [] }").unwrap();
        assert_eq!(script.rules[0].sizes, Vec::new());
    }

    #[test]
    fn unknown_identifier_in_a_condition_is_rejected() {
        for name in [
            "limpers",
            "flats",
            "squeeze",
            "open_cold_calls",
            "preflop_participant",
            "in_position_to_last_aggressor",
        ] {
            let source = format!("flop when {name} {{ checkdown }}");
            let error = compile(&source).unwrap_err();
            assert!(error.message.contains(name), "{name}: {}", error.message);
        }
    }

    #[test]
    fn type_errors_are_rejected() {
        assert!(compile("flop when spr { checkdown }").is_err());
        assert!(compile("flop when paired > 1 { checkdown }").is_err());
        assert!(compile("flop when high_card >= \"A\" { checkdown }").is_err());
        assert!(compile("flop when cbet == 3 { checkdown }").is_err());
    }

    #[test]
    fn errors_carry_a_plausible_line_number() {
        let source = "flop {\n  when spr {\n    checkdown\n  }\n}";
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, 2);

        let source = "param cb = 33\n\nflop {\n  unknown_var\n}";
        let error = compile(source).unwrap_err();
        assert_eq!(error.line, 4);
    }

    #[test]
    fn param_description_is_the_comment_run_directly_above() {
        let source = "# c-bet size (pot %)\n# second line\nparam cb = 33\nflop { checkdown }";
        let script = compile(source).unwrap();
        assert_eq!(
            script.params[0].description.as_deref(),
            Some("c-bet size (pot %)\nsecond line")
        );
    }

    #[test]
    fn trailing_comment_on_the_param_line_is_not_a_description() {
        let source = "param cb = 33 # trailing, not a description\nflop { checkdown }";
        let script = compile(source).unwrap();
        assert_eq!(script.params[0].description, None);
    }
}
