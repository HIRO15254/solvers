//! Flattens a parsed street-block list into the flat [`Rule`] list a
//! compiled [`Script`](super::Script) carries.
//!
//! The walk is pre-order: outer blocks are visited before the statements
//! nested inside them, so a rule from an outer `when` always lands before
//! the rules its inner `when`s produce. Rules come out in exactly that
//! order, and source order is the whole priority model -- there is no
//! separate priority field (see the spec's "flattening" section).

use super::ast::{Rule, StmtAst, StreetBlockAst};
use super::cond::{Condition, and_simplify};

/// Flattens every street block into one rule list, in source order.
pub(crate) fn lower(blocks: Vec<StreetBlockAst>) -> Vec<Rule> {
    let mut rules = Vec::new();
    for block in blocks {
        // A street block with no shorthand `when` starts from `Const(true)`
        // rather than `None`, so a statement directly under it -- with no
        // enclosing condition at all -- gets exactly `Condition::Const(true)`
        // per the spec, not a vacuous `true && true`.
        let base = block.condition.unwrap_or(Condition::Const(true));
        lower_body(&block.streets, &base, &block.body, &mut rules);
    }
    rules
}

fn lower_body(
    streets: &[crate::Street],
    enclosing: &Condition,
    body: &[StmtAst],
    rules: &mut Vec<Rule>,
) {
    for stmt in body {
        match stmt {
            StmtAst::Action {
                effect,
                action,
                sizes,
            } => {
                for &street in streets {
                    rules.push(Rule {
                        street,
                        condition: enclosing.clone(),
                        effect: *effect,
                        action: *action,
                        sizes: sizes.clone(),
                    });
                }
            }
            StmtAst::When { condition, body } => {
                let combined = and_simplify(enclosing.clone(), condition.clone());
                lower_body(streets, &combined, body, rules);
            }
            StmtAst::If { arms, else_body } => {
                // Each arm's condition is its own condition ANDed with the
                // negation of every earlier arm's condition, so the arms
                // are syntactically exclusive -- nobody has to remember to
                // write `!a && !b` themselves.
                let mut none_matched_yet = Condition::Const(true);
                for (condition, arm_body) in arms {
                    let arm_condition = and_simplify(
                        and_simplify(enclosing.clone(), none_matched_yet.clone()),
                        condition.clone(),
                    );
                    lower_body(streets, &arm_condition, arm_body, rules);
                    none_matched_yet = and_simplify(
                        none_matched_yet,
                        Condition::Not(Box::new(condition.clone())),
                    );
                }
                if let Some(else_body) = else_body {
                    let else_condition = and_simplify(enclosing.clone(), none_matched_yet);
                    lower_body(streets, &else_condition, else_body, rules);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ast::{ActionKind, Effect};
    use super::super::cond::{CmpOp, Literal, PreviousAggressor, RuleContext, Var};
    use super::*;
    use crate::{BoardFacts, SizeSpec, Street};

    fn truth(var: Var) -> Condition {
        Condition::Truth(var)
    }

    fn cmp(var: Var, op: CmpOp, value: f64) -> Condition {
        Condition::Compare {
            var,
            op,
            value: Literal::Number(value),
        }
    }

    fn action(effect: Effect, action_kind: ActionKind, sizes: Vec<SizeSpec>) -> StmtAst {
        StmtAst::Action {
            effect,
            action: Some(action_kind),
            sizes,
        }
    }

    fn ctx(aggressions: u32, spr: f64) -> RuleContext {
        RuleContext {
            aggressions,
            in_position: true,
            spr,
            pot: 100.0,
            to_call: 0.0,
            previous_aggressor: PreviousAggressor::None,
            board: BoardFacts::default(),
        }
    }

    /// The spec's nested-`when` worked example: `flop when cbet { replace
    /// bet [cb]; when wet { replace bet [cb,75] }; when spr<=3 { replace
    /// raise [a] } }` must lower to exactly the three rules in its table.
    #[test]
    fn nested_when_matches_the_spec_worked_example() {
        let cb = SizeSpec::PotAfterCall { fraction: 0.33 };
        let cb75 = SizeSpec::PotAfterCall { fraction: 0.75 };
        let allin = SizeSpec::AllIn;
        let wet = Condition::Or(
            Box::new(truth(Var::FlushPossible)),
            Box::new(truth(Var::StraightPossible)),
        );
        let block = StreetBlockAst {
            streets: vec![Street::Flop],
            condition: Some(truth(Var::Cbet)),
            body: vec![
                action(Effect::Replace, ActionKind::Bet, vec![cb]),
                StmtAst::When {
                    condition: wet.clone(),
                    body: vec![action(Effect::Replace, ActionKind::Bet, vec![cb, cb75])],
                },
                StmtAst::When {
                    condition: cmp(Var::Spr, CmpOp::Le, 3.0),
                    body: vec![action(Effect::Replace, ActionKind::Raise, vec![allin])],
                },
            ],
        };
        let rules = lower(vec![block]);
        assert_eq!(rules.len(), 3);

        // Row 1: `cbet`.
        assert_eq!(rules[0].effect, Effect::Replace);
        assert_eq!(rules[0].action, Some(ActionKind::Bet));
        assert_eq!(rules[0].sizes, vec![cb]);
        let mut context = ctx(0, 5.0);
        context.previous_aggressor = PreviousAggressor::Actor;
        assert!(rules[0].condition.eval(&context));

        // Row 2: `cbet && (flush_possible || straight_possible)`.
        assert_eq!(rules[1].sizes, vec![cb, cb75]);
        let mut wet_board_ctx = context;
        wet_board_ctx.board = BoardFacts::new(&[
            "2h".parse().unwrap(),
            "5h".parse().unwrap(),
            "9h".parse().unwrap(),
        ]);
        assert!(rules[1].condition.eval(&wet_board_ctx));
        let mut dry_board_ctx = context;
        dry_board_ctx.board = BoardFacts::new(&[
            "2h".parse().unwrap(),
            "7d".parse().unwrap(),
            "Kc".parse().unwrap(),
        ]);
        assert!(!rules[1].condition.eval(&dry_board_ctx));

        // Row 3: `cbet && spr <= 3`.
        assert_eq!(rules[2].action, Some(ActionKind::Raise));
        assert_eq!(rules[2].sizes, vec![allin]);
        let mut shallow_ctx = ctx(0, 2.0);
        shallow_ctx.previous_aggressor = PreviousAggressor::Actor;
        assert!(rules[2].condition.eval(&shallow_ctx));
        let mut deep_ctx = ctx(0, 5.0);
        deep_ctx.previous_aggressor = PreviousAggressor::Actor;
        assert!(!rules[2].condition.eval(&deep_ctx));
    }

    /// The spec's `if` / `else if` / `else` worked example: four mutually
    /// exclusive rows, each ANDed with the negation of every earlier arm.
    #[test]
    fn if_else_chain_matches_the_spec_worked_example() {
        let size25 = SizeSpec::PotAfterCall { fraction: 0.25 };
        let size33 = SizeSpec::PotAfterCall { fraction: 0.33 };
        let size66 = SizeSpec::PotAfterCall { fraction: 0.66 };
        let size75 = SizeSpec::PotAfterCall { fraction: 0.75 };
        let wet = Condition::Or(
            Box::new(truth(Var::FlushPossible)),
            Box::new(truth(Var::StraightPossible)),
        );
        let block = StreetBlockAst {
            streets: vec![Street::Flop],
            condition: Some(truth(Var::Cbet)),
            body: vec![StmtAst::If {
                arms: vec![
                    (
                        truth(Var::Paired),
                        vec![action(
                            Effect::Replace,
                            ActionKind::Bet,
                            vec![size25, size75],
                        )],
                    ),
                    (
                        truth(Var::Monotone),
                        vec![action(Effect::Replace, ActionKind::Bet, vec![size33])],
                    ),
                    (
                        wet,
                        vec![action(Effect::Replace, ActionKind::Bet, vec![size66])],
                    ),
                ],
                else_body: Some(vec![action(Effect::Replace, ActionKind::Bet, vec![size25])]),
            }],
        };
        let rules = lower(vec![block]);
        assert_eq!(rules.len(), 4);

        let mut cbet_ctx = ctx(0, 5.0);
        cbet_ctx.previous_aggressor = PreviousAggressor::Actor;

        let paired_board = BoardFacts::new(&[
            "9h".parse().unwrap(),
            "9d".parse().unwrap(),
            "2c".parse().unwrap(),
        ]);
        let monotone_board = BoardFacts::new(&[
            "2h".parse().unwrap(),
            "5h".parse().unwrap(),
            "9h".parse().unwrap(),
        ]);
        let wet_rainbow_board = BoardFacts::new(&[
            "9h".parse().unwrap(),
            "7d".parse().unwrap(),
            "5c".parse().unwrap(),
        ]);
        let dry_board = BoardFacts::new(&[
            "2h".parse().unwrap(),
            "7d".parse().unwrap(),
            "Kc".parse().unwrap(),
        ]);

        let eval_all = |board: BoardFacts| -> Vec<bool> {
            let mut context = cbet_ctx;
            context.board = board;
            rules.iter().map(|r| r.condition.eval(&context)).collect()
        };

        assert_eq!(eval_all(paired_board), vec![true, false, false, false]);
        assert_eq!(eval_all(monotone_board), vec![false, true, false, false]);
        assert_eq!(eval_all(wet_rainbow_board), vec![false, false, true, false]);
        assert_eq!(eval_all(dry_board), vec![false, false, false, true]);

        assert_eq!(rules[0].sizes, vec![size25, size75]);
        assert_eq!(rules[1].sizes, vec![size33]);
        assert_eq!(rules[2].sizes, vec![size66]);
        assert_eq!(rules[3].sizes, vec![size25]);
    }

    #[test]
    fn if_with_no_else_leaves_unmatched_nodes_with_no_extra_rule() {
        let block = StreetBlockAst {
            streets: vec![Street::Flop],
            condition: None,
            body: vec![StmtAst::If {
                arms: vec![(
                    truth(Var::Paired),
                    vec![action(Effect::Remove, ActionKind::Bet, vec![])],
                )],
                else_body: None,
            }],
        };
        let rules = lower(vec![block]);
        assert_eq!(rules.len(), 1);
        let mut context = ctx(0, 5.0);
        context.board = BoardFacts::new(&[
            "2h".parse().unwrap(),
            "7d".parse().unwrap(),
            "Kc".parse().unwrap(),
        ]);
        assert!(!rules[0].condition.eval(&context));
    }

    /// An unconditioned statement (no enclosing `when`) gets exactly
    /// `Condition::Const(true)`, not a chain of vacuous `true && true`.
    #[test]
    fn unconditioned_statement_gets_const_true() {
        let block = StreetBlockAst {
            streets: vec![Street::Turn],
            condition: None,
            body: vec![StmtAst::Action {
                effect: Effect::Checkdown,
                action: None,
                sizes: vec![],
            }],
        };
        let rules = lower(vec![block]);
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].condition, Condition::Const(true));
    }

    /// Depth >= 3: a `when` nested inside a `when` nested inside a `when`.
    #[test]
    fn nesting_depth_of_three_ands_every_level() {
        let block = StreetBlockAst {
            streets: vec![Street::River],
            condition: None,
            body: vec![StmtAst::When {
                condition: truth(Var::Unopened),
                body: vec![StmtAst::When {
                    condition: cmp(Var::Spr, CmpOp::Le, 2.0),
                    body: vec![StmtAst::When {
                        condition: truth(Var::Paired),
                        body: vec![action(
                            Effect::Force,
                            ActionKind::Bet,
                            vec![SizeSpec::AllIn],
                        )],
                    }],
                }],
            }],
        };
        let rules = lower(vec![block]);
        assert_eq!(rules.len(), 1);

        let mut matching = ctx(0, 1.5);
        matching.board = BoardFacts::new(&[
            "9h".parse().unwrap(),
            "9d".parse().unwrap(),
            "2c".parse().unwrap(),
        ]);
        assert!(rules[0].condition.eval(&matching));

        let mut wrong_spr = matching;
        wrong_spr.spr = 5.0;
        assert!(!rules[0].condition.eval(&wrong_spr));
    }

    /// The same street named in two separate top-level blocks is fine (the
    /// spec's own complete example has `turn when unopened {...}` and
    /// `turn, river when monotone {...}`); each block lowers independently.
    #[test]
    fn same_street_in_two_separate_blocks_lowers_independently() {
        let first = StreetBlockAst {
            streets: vec![Street::Turn],
            condition: Some(truth(Var::Unopened)),
            body: vec![action(
                Effect::Replace,
                ActionKind::Bet,
                vec![SizeSpec::AllIn],
            )],
        };
        let second = StreetBlockAst {
            streets: vec![Street::Turn, Street::River],
            condition: Some(truth(Var::Monotone)),
            body: vec![StmtAst::Action {
                effect: Effect::Checkdown,
                action: None,
                sizes: vec![],
            }],
        };
        let rules = lower(vec![first, second]);
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].street, Street::Turn);
        assert_eq!(rules[1].street, Street::Turn);
        assert_eq!(rules[2].street, Street::River);
        assert_eq!(rules[1].effect, Effect::Checkdown);
    }
}
