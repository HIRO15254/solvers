//! Tokenizer for the postflop tree script.
//!
//! One pass over `char`s (not bytes), tracking 1-based line numbers. Working
//! in `char`s rather than bytes fixes two bugs the older multiway condition
//! parser (`crates/multiway/src/tree_rules.rs`) has: it strips comments with
//! `line.split('#')`, so a `#` inside a string literal truncates the line,
//! and it casts `bytes[index] as char`, so a non-ASCII byte gets mangled
//! into latin-1 instead of producing an error. Iterating `char`s and lexing
//! strings as their own token kind avoids both: a `#` inside a `"..."`
//! literal is just string content, and a non-ASCII character that is not
//! part of a string or comment falls through every token rule and is
//! reported as the unexpected character it is.

use super::ScriptError;

/// One lexical token, tagged with the 1-based source line it started on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token {
    pub kind: TokenKind,
    pub line: usize,
}

/// The kinds of token the tree script grammar is built from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TokenKind {
    /// A maximal run of `[A-Za-z0-9_.%-]`. This single rule covers
    /// identifiers (`cbet`), numbers (`33`, `2.5`, `-1`), and every size
    /// literal including `80%effective`, `3x`, `1e`, `a`, `min` -- so the
    /// tokenizer needs no lexer modes to tell them apart; that is left to
    /// the parser, which knows what it is expecting.
    Word(String),
    /// A `"..."` string literal. No escapes, so the closing quote is always
    /// the next `"` byte-for-byte; unterminated is an error.
    Str(String),
    /// One of the fixed operator/bracket spellings, matched longest-first
    /// so `==` is never split into two `=` tokens.
    Punct(&'static str),
    /// A `#` to end-of-line comment, kept as a token (rather than discarded
    /// during lexing) because a `param`'s description is the run of
    /// comment lines directly above its declaration.
    Comment(String),
}

/// Punctuation tokens, longest match first so a greedy scan never splits
/// `&&`, `||`, `<=`, `>=`, `==`, or `!=` into two single-character tokens.
const PUNCTS: &[&str] = &[
    "&&", "||", "<=", ">=", "==", "!=", "{", "}", "[", "]", "(", ")", ",", "=", "<", ">", "!",
];

/// A word character per the grammar: `[A-Za-z0-9_.%-]`. Deliberately ASCII
/// only -- a non-ASCII character is neither whitespace, `#`, `"`, a punct,
/// nor a word character, so it falls through to the "unexpected character"
/// error instead of being silently absorbed or mangled.
fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '%' || c == '-'
}

/// Tokenizes a whole tree-script source. Comments are retained as
/// [`TokenKind::Comment`] tokens; the parser strips them once it has
/// harvested any `param` descriptions from them.
pub(crate) fn tokenize(source: &str) -> Result<Vec<Token>, ScriptError> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let mut line = 1usize;

    while i < chars.len() {
        let c = chars[i];
        if c == '\n' {
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '#' {
            let start = i;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Comment(chars[start..i].iter().collect()),
                line,
            });
            continue;
        }
        if c == '"' {
            let string_line = line;
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '"' {
                // A raw newline inside an unterminated string still counts
                // towards line numbers for whatever comes after, in case
                // the caller wants to keep reporting sane lines past the
                // error (tests only assert the reported line, but nothing
                // else should silently desync).
                if chars[i] == '\n' {
                    line += 1;
                }
                i += 1;
            }
            if i >= chars.len() {
                return Err(ScriptError {
                    line: string_line,
                    message: "unterminated string literal".to_string(),
                });
            }
            tokens.push(Token {
                kind: TokenKind::Str(chars[start..i].iter().collect()),
                line: string_line,
            });
            i += 1; // skip the closing quote
            continue;
        }
        if is_word_char(c) {
            let start = i;
            let word_line = line;
            while i < chars.len() && is_word_char(chars[i]) {
                i += 1;
            }
            tokens.push(Token {
                kind: TokenKind::Word(chars[start..i].iter().collect()),
                line: word_line,
            });
            continue;
        }
        if let Some(&punct) = PUNCTS.iter().find(|p| starts_with_at(&chars, i, p)) {
            tokens.push(Token {
                kind: TokenKind::Punct(punct),
                line,
            });
            i += punct.chars().count();
            continue;
        }
        return Err(ScriptError {
            line,
            message: format!("unexpected character {c:?}"),
        });
    }

    Ok(tokens)
}

fn starts_with_at(chars: &[char], i: usize, needle: &str) -> bool {
    let mut j = i;
    for expected in needle.chars() {
        match chars.get(j) {
            Some(&c) if c == expected => j += 1,
            _ => return false,
        }
    }
    true
}

/// True when the token at `pos` is the given punctuation.
pub(crate) fn is_punct(tokens: &[Token], pos: usize, p: &str) -> bool {
    matches!(tokens.get(pos).map(|t| &t.kind), Some(TokenKind::Punct(found)) if *found == p)
}

/// True when the token at `pos` is the given bare word (used for keywords).
pub(crate) fn is_word(tokens: &[Token], pos: usize, w: &str) -> bool {
    matches!(tokens.get(pos).map(|t| &t.kind), Some(TokenKind::Word(found)) if found == w)
}

/// The line of the token at `pos`, or the line just past the last token if
/// `pos` is at or beyond the end -- used so an "unexpected end of input"
/// error still names a plausible line rather than panicking.
pub(crate) fn line_at(tokens: &[Token], pos: usize) -> usize {
    tokens
        .get(pos)
        .or_else(|| tokens.last())
        .map(|t| t.line)
        .unwrap_or(1)
}

/// Consumes the given punctuation or reports the line it was expected at.
pub(crate) fn expect_punct(
    tokens: &[Token],
    pos: &mut usize,
    p: &'static str,
) -> Result<(), ScriptError> {
    if is_punct(tokens, *pos, p) {
        *pos += 1;
        Ok(())
    } else {
        Err(ScriptError {
            line: line_at(tokens, *pos),
            message: format!("expected {p:?}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(tokens: &[Token]) -> Vec<&str> {
        tokens
            .iter()
            .filter_map(|t| match &t.kind {
                TokenKind::Word(w) => Some(w.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn word_rule_covers_identifiers_numbers_and_size_literals() {
        let tokens = tokenize("cbet 33 2.5 -1 80%effective 3x 1e a min").unwrap();
        assert_eq!(
            words(&tokens),
            vec![
                "cbet",
                "33",
                "2.5",
                "-1",
                "80%effective",
                "3x",
                "1e",
                "a",
                "min"
            ]
        );
    }

    #[test]
    fn puncts_match_longest_first() {
        let tokens = tokenize("&& || <= >= == != < > ! { } [ ] ( ) , =").unwrap();
        let kinds: Vec<&str> = tokens
            .iter()
            .map(|t| match &t.kind {
                TokenKind::Punct(p) => *p,
                _ => panic!("expected punct, got {t:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                "&&", "||", "<=", ">=", "==", "!=", "<", ">", "!", "{", "}", "[", "]", "(", ")",
                ",", "="
            ]
        );
    }

    #[test]
    fn double_equals_wins_over_two_single_equals() {
        let tokens = tokenize("a==b").unwrap();
        assert!(matches!(tokens[1].kind, TokenKind::Punct("==")));
    }

    /// The multiway lexer bug this must not reproduce: `line.split('#')`
    /// truncates a line at a `#` even inside a string literal.
    #[test]
    fn hash_inside_a_string_literal_is_not_a_comment() {
        let tokens = tokenize(r#"define x = high_card == "A#weird""#).unwrap();
        let string_token = tokens
            .iter()
            .find_map(|t| match &t.kind {
                TokenKind::Str(s) => Some(s.as_str()),
                _ => None,
            })
            .expect("expected a string token");
        assert_eq!(string_token, "A#weird");
        assert!(
            !tokens
                .iter()
                .any(|t| matches!(t.kind, TokenKind::Comment(_))),
            "a '#' inside a string must not start a comment"
        );
    }

    #[test]
    fn comment_runs_to_end_of_line() {
        let tokens = tokenize("param cb = 33 # c-bet size\nflop { checkdown }").unwrap();
        let comment = tokens
            .iter()
            .find_map(|t| match &t.kind {
                TokenKind::Comment(c) => Some(c.as_str()),
                _ => None,
            })
            .unwrap();
        assert_eq!(comment, "# c-bet size");
    }

    /// The multiway lexer's other bug: `bytes[index] as char` mangles a
    /// non-ASCII byte into latin-1 instead of erroring. Iterating `char`s
    /// means a non-ASCII character outside a string/comment is simply not a
    /// word character, a punct, or whitespace, so it must be reported as an
    /// error naming the character and line -- never silently reinterpreted.
    #[test]
    fn non_ascii_character_errors_with_a_line_number_instead_of_being_mangled() {
        let error = tokenize("flop {\n  when pair\u{00e9} { checkdown }\n}").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains('é'), "{}", error.message);
    }

    #[test]
    fn non_ascii_inside_a_string_or_comment_is_fine() {
        assert!(tokenize("# caf\u{00e9}\nflop { checkdown }").is_ok());
        let source = format!("define x = high_card == \"caf{}\"", '\u{00e9}');
        assert!(tokenize(&source).is_ok());
    }

    #[test]
    fn unterminated_string_is_an_error_at_its_start_line() {
        let error = tokenize("define x =\n  \"unterminated").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("unterminated"));
    }

    #[test]
    fn line_numbers_track_newlines_across_the_source() {
        let tokens = tokenize("flop {\n  checkdown\n}").unwrap();
        let checkdown = tokens
            .iter()
            .find(|t| matches!(&t.kind, TokenKind::Word(w) if w == "checkdown"))
            .unwrap();
        assert_eq!(checkdown.line, 2);
    }
}
