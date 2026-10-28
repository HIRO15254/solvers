//! Common-input tree fixtures for tests and benchmarks.
use crate::StreetTree;
use crate::input::NlhStreetRules;
use nlh::script::{ActionKind, CmpOp, Condition, Effect, Literal, Rule};
use nlh::{SizeSpec, Street};
use spot::TreeVar;

pub fn street_rules(rules: Vec<Rule<TreeVar>>) -> Box<NlhStreetRules> {
    Box::new(NlhStreetRules {
        rules,
        ..Default::default()
    })
}

/// Append the fixture's explicit aggression-limiting script rules last.
pub fn limit_aggressions(rules: &mut Vec<Rule<TreeVar>>, street: Street, limit: u32) {
    let actions: &[ActionKind] = if limit == 0 {
        &[ActionKind::Bet, ActionKind::Raise]
    } else {
        &[ActionKind::Raise]
    };
    for &action in actions {
        rules.push(Rule {
            street,
            condition: Condition::Compare {
                var: TreeVar::Aggressions,
                op: CmpOp::Ge,
                value: Literal::Number(f64::from(limit)),
            },
            effect: Effect::Remove,
            action: Some(action),
            sizes: Vec::new(),
        });
    }
}

/// `add bet [a]` and `add raise [a]` on `street`. Placed first, every menu
/// starts with the all-in that later rules may keep or remove.
pub fn allin_rules(street: Street) -> Vec<Rule<TreeVar>> {
    [ActionKind::Bet, ActionKind::Raise]
        .into_iter()
        .map(|action| Rule {
            street,
            condition: Condition::Const(true),
            effect: Effect::Add,
            action: Some(action),
            sizes: vec![SizeSpec::AllIn],
        })
        .collect()
}

pub fn from_script(street: Street, rules: &[Rule<TreeVar>], aggression_limit: u32) -> StreetTree {
    let mut rules: Vec<_> = rules
        .iter()
        .filter(|rule| rule.street == street)
        .cloned()
        .collect();
    limit_aggressions(&mut rules, street, aggression_limit);
    StreetTree {
        nlh_rules: street_rules(rules),
    }
}

pub fn pot_fractions(oop: &[f64], ip: &[f64], aggression_limit: u32) -> StreetTree {
    let mut rules = Vec::new();
    for action in [ActionKind::Bet, ActionKind::Raise] {
        for (in_position, fractions) in [(false, oop), (true, ip)] {
            let actor = if in_position {
                Condition::Truth(TreeVar::InPosition)
            } else {
                Condition::Not(Box::new(Condition::Truth(TreeVar::InPosition)))
            };
            let condition = if action == ActionKind::Raise {
                Condition::And(
                    Box::new(actor),
                    Box::new(Condition::Not(Box::new(Condition::Truth(
                        TreeVar::Unopened,
                    )))),
                )
            } else {
                actor
            };
            rules.push(Rule {
                street: Street::Flop,
                condition,
                effect: Effect::Add,
                action: Some(action),
                sizes: fractions
                    .iter()
                    .map(|&fraction| SizeSpec::PotAfterCall { fraction })
                    .collect(),
            });
        }
    }
    limit_aggressions(&mut rules, Street::Flop, aggression_limit);
    StreetTree {
        nlh_rules: street_rules(rules),
    }
}
