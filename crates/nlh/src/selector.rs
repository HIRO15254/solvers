/// Convert a displayed action label into its tree-history token.
pub fn history_token(label: &str) -> String {
    match label {
        "check" => "x".to_string(),
        "fold" => "f".to_string(),
        "call" => "c".to_string(),
        _ => {
            let amount = label.rsplit(' ').next().unwrap_or(label);
            format!("r{amount}")
        }
    }
}

/// Resolve a typed action label or numeric index.
pub fn resolve_action(actions: &[&str], arg: &str) -> Result<usize, String> {
    if let Some(p) = actions.iter().position(|a| *a == arg) {
        return Ok(p);
    }
    match arg.parse::<usize>() {
        Ok(idx) if idx < actions.len() => Ok(idx),
        Ok(idx) => Err(format!(
            "action index {idx} out of range (0..{})",
            actions.len()
        )),
        Err(_) => Err(format!("unknown action {arg:?}")),
    }
}

/// Resolve labels by their rounded chip amount, preserving action kind.
pub fn resolve_scaled_action(
    actions: &[&str],
    arg: &str,
    scale: crate::ChipScale,
    rounding: crate::Rounding,
) -> Result<usize, String> {
    for prefix in ["bet ", "raise to "] {
        if let Some(wanted) = arg.strip_prefix(prefix)
            && let Some(pos) = actions.iter().position(|label| {
                label.strip_prefix(prefix).is_some_and(|actual| {
                    history_token_matches(
                        &format!("r{actual}"),
                        &format!("r{wanted}"),
                        scale,
                        rounding,
                    )
                })
            })
        {
            return Ok(pos);
        }
    }
    resolve_action(actions, arg)
}

/// Compare history actions by chip amount, rounding user-written BB amounts.
pub fn history_token_matches(
    actual: &str,
    wanted: &str,
    scale: crate::ChipScale,
    rounding: crate::Rounding,
) -> bool {
    if actual == wanted {
        return true;
    }
    fn amount(
        token: &str,
        scale: crate::ChipScale,
        rounding: crate::Rounding,
    ) -> Option<crate::MwChips> {
        let text = token.strip_prefix('r')?;
        if let Some(chips) = text.strip_suffix('c') {
            chips.parse::<u64>().ok().map(crate::MwChips)
        } else {
            scale
                .parse_bb_rounded(text.strip_suffix("bb").unwrap_or(text), rounding)
                .ok()
                .map(|(chips, _)| chips)
        }
    }
    match (
        amount(actual, scale, rounding),
        amount(wanted, scale, rounding),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Parse ambiguous `rNc` as either a chip amount or a BB amount followed by
/// a call, preferring the table's canonical spelling. `max_depth` is an
/// exclusive bound on the minimum number of history steps.
pub fn selector_history_steps(
    selector: &str,
    scale: crate::ChipScale,
    max_depth: usize,
) -> Vec<Vec<&str>> {
    // Bound both the number of tokenizations and recursive parser depth.
    // Sixteen ambiguous amounts permit 65,536 possible interpretations.
    const MAX_AMBIGUOUS: usize = 16;
    const MAX_SELECTOR_BYTES: usize = 4096;
    if selector.len() > MAX_SELECTOR_BYTES {
        return Vec::new();
    }
    let bytes = selector.as_bytes();
    let mut ambiguous = 0;
    let mut minimum_steps = 0;
    let mut i = 0;
    while i < bytes.len() {
        minimum_steps += 1;
        if minimum_steps >= max_depth {
            return Vec::new();
        }
        if bytes[i] == b'[' {
            match bytes[i..].iter().position(|&b| b == b']') {
                Some(end) => i += end + 1,
                None => return Vec::new(),
            }
        } else if bytes[i] == b'r' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if bytes.get(i..i + 2) == Some(b"bb") {
                i += 2;
            }
            if bytes.get(i) == Some(&b'c') {
                ambiguous += 1;
                if ambiguous > MAX_AMBIGUOUS {
                    return Vec::new();
                }
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    fn parse<'a>(
        rest: &'a str,
        chips_first: bool,
        steps: &mut Vec<&'a str>,
        out: &mut Vec<Vec<&'a str>>,
    ) {
        if rest.is_empty() {
            out.push(steps.clone());
            return;
        }
        let len = match rest.as_bytes()[0] {
            b'x' | b'c' | b'f' => 1,
            b'r' => {
                1 + rest[1..]
                    .bytes()
                    .take_while(|b| b.is_ascii_digit() || *b == b'.')
                    .count()
            }
            b'[' => match rest.find(']') {
                Some(end) => end + 1,
                None => return,
            },
            _ => return,
        };
        let len = if rest.starts_with('r') && rest[len..].starts_with("bb") {
            len + 2
        } else {
            len
        };
        let ambiguous = rest.starts_with('r') && rest.as_bytes().get(len) == Some(&b'c');
        for suffix in [chips_first, !chips_first] {
            if suffix && !ambiguous {
                continue;
            }
            let end = len + usize::from(suffix);
            steps.push(&rest[..end]);
            parse(&rest[end..], chips_first, steps, out);
            steps.pop();
        }
    }
    let mut out = Vec::new();
    parse(selector, !scale.is_decimal(), &mut Vec::new(), &mut out);
    out
}
