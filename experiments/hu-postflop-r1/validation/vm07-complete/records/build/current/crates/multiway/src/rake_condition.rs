#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RakeConditionContext {
    pub flop_dealt: bool,
    pub showdown: bool,
    pub players_dealt: u8,
    pub players_saw_flop: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledRakeCondition {
    bits: [u64; 5],
}

impl CompiledRakeCondition {
    pub fn matches(self, context: RakeConditionContext) -> bool {
        let index = context_index(context);
        self.bits[index / 64] & (1_u64 << (index % 64)) != 0
    }

    /// True when the condition matches no context in the compiled truth
    /// table at all (e.g. `players_dealt > 20`), so it can never fire.
    pub fn is_never(self) -> bool {
        self.bits.iter().all(|word| *word == 0)
    }
}

pub fn compile(source: &str) -> Result<CompiledRakeCondition, String> {
    let probe = RakeConditionContext {
        flop_dealt: false,
        showdown: false,
        players_dealt: 2,
        players_saw_flop: 0,
    };
    evaluate(source, probe)?;
    let mut bits = [0_u64; 5];
    for flop_dealt in [false, true] {
        for showdown in [false, true] {
            for players_dealt in 2..=9 {
                for players_saw_flop in 0..=9 {
                    let context = RakeConditionContext {
                        flop_dealt,
                        showdown,
                        players_dealt,
                        players_saw_flop,
                    };
                    if evaluate(source, context)? {
                        let index = context_index(context);
                        bits[index / 64] |= 1_u64 << (index % 64);
                    }
                }
            }
        }
    }
    Ok(CompiledRakeCondition { bits })
}

fn context_index(context: RakeConditionContext) -> usize {
    ((((context.flop_dealt as usize) * 2 + context.showdown as usize) * 8
        + usize::from(context.players_dealt.saturating_sub(2)))
        * 10)
        + usize::from(context.players_saw_flop)
}

fn evaluate(source: &str, context: RakeConditionContext) -> Result<bool, String> {
    let mut parser = Parser {
        source: source.as_bytes(),
        position: 0,
        context,
    };
    let value = parser.parse_or()?;
    parser.skip_space();
    if parser.position != parser.source.len() {
        return Err(format!(
            "unexpected token at byte {} in rake condition",
            parser.position
        ));
    }
    Ok(value)
}

struct Parser<'a> {
    source: &'a [u8],
    position: usize,
    context: RakeConditionContext,
}

impl Parser<'_> {
    fn parse_or(&mut self) -> Result<bool, String> {
        let mut value = self.parse_and()?;
        while self.consume("||") {
            let right = self.parse_and()?;
            value = value || right;
        }
        Ok(value)
    }

    fn parse_and(&mut self) -> Result<bool, String> {
        let mut value = self.parse_unary()?;
        while self.consume("&&") {
            let right = self.parse_unary()?;
            value = value && right;
        }
        Ok(value)
    }

    fn parse_unary(&mut self) -> Result<bool, String> {
        if self.consume("!") {
            return Ok(!self.parse_unary()?);
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<bool, String> {
        if self.consume("(") {
            let value = self.parse_or()?;
            if !self.consume(")") {
                return Err("missing ')' in rake condition".into());
            }
            return Ok(value);
        }
        let identifier = self.identifier()?;
        match identifier.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            "flop_dealt" => Ok(self.context.flop_dealt),
            "showdown" => Ok(self.context.showdown),
            "won_without_showdown" => Ok(!self.context.showdown),
            "players_dealt" => {
                let left = i64::from(self.context.players_dealt);
                self.numeric_comparison(left)
            }
            "players_saw_flop" => {
                let left = i64::from(self.context.players_saw_flop);
                self.numeric_comparison(left)
            }
            other => Err(format!("unknown rake condition identifier {other:?}")),
        }
    }

    fn numeric_comparison(&mut self, left: i64) -> Result<bool, String> {
        let operator = ["<=", ">=", "==", "!=", "<", ">"]
            .into_iter()
            .find(|operator| self.consume(operator))
            .ok_or_else(|| "integer rake condition requires a comparison".to_string())?;
        self.skip_space();
        let start = self.position;
        while self
            .source
            .get(self.position)
            .is_some_and(u8::is_ascii_digit)
        {
            self.position += 1;
        }
        if start == self.position {
            return Err("rake condition comparison requires an integer".into());
        }
        let right = std::str::from_utf8(&self.source[start..self.position])
            .map_err(|_| "rake condition is not UTF-8".to_string())?
            .parse::<i64>()
            .map_err(|_| "invalid rake condition integer".to_string())?;
        Ok(match operator {
            "<=" => left <= right,
            ">=" => left >= right,
            "==" => left == right,
            "!=" => left != right,
            "<" => left < right,
            ">" => left > right,
            _ => unreachable!(),
        })
    }

    fn identifier(&mut self) -> Result<String, String> {
        self.skip_space();
        let start = self.position;
        while self
            .source
            .get(self.position)
            .is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
        {
            self.position += 1;
        }
        if start == self.position {
            return Err(format!(
                "expected rake condition expression at byte {}",
                self.position
            ));
        }
        std::str::from_utf8(&self.source[start..self.position])
            .map(str::to_owned)
            .map_err(|_| "rake condition is not UTF-8".to_string())
    }

    fn consume(&mut self, token: &str) -> bool {
        self.skip_space();
        let bytes = token.as_bytes();
        if self.source[self.position..].starts_with(bytes) {
            self.position += bytes.len();
            true
        } else {
            false
        }
    }

    fn skip_space(&mut self) {
        while self
            .source
            .get(self.position)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.position += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_and_integer_conditions_compile_to_a_truth_table() {
        let condition =
            compile("flop_dealt && players_dealt >= 6 && (showdown || players_saw_flop == 4)")
                .unwrap();
        assert!(condition.matches(RakeConditionContext {
            flop_dealt: true,
            showdown: false,
            players_dealt: 6,
            players_saw_flop: 4,
        }));
        assert!(!condition.matches(RakeConditionContext {
            flop_dealt: true,
            showdown: false,
            players_dealt: 5,
            players_saw_flop: 4,
        }));
        assert!(compile("cards_seen > 3").is_err());
    }

    #[test]
    fn is_never_detects_an_unsatisfiable_condition() {
        assert!(compile("players_dealt > 20").unwrap().is_never());
        assert!(!compile("true").unwrap().is_never());
        assert!(!compile("flop_dealt").unwrap().is_never());
    }
}
