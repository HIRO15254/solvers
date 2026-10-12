//! Common-input tree fixtures for tests and benchmarks.
use crate::StreetTree;
use crate::input::NlhStreetRules;
use nlh::script::{ActionKind, Condition, Effect, Rule};
use nlh::{SizeSpec, Street};
use spot::TreeVar;

pub fn street_rules(rules: Vec<Rule<TreeVar>>) -> Box<NlhStreetRules> {
    Box::new(NlhStreetRules {
        rules,
        ..Default::default()
    })
}

pub fn from_script(
    street: Street,
    rules: &[Rule<TreeVar>],
    max_aggressive_actions: u32,
    include_allin: bool,
    allin_threshold: Option<f64>,
) -> StreetTree {
    StreetTree {
        nlh_rules: street_rules(
            rules
                .iter()
                .filter(|rule| rule.street == street)
                .cloned()
                .collect(),
        ),
        max_aggressive_actions,
        include_allin,
        allin_threshold,
    }
}

pub fn pot_fractions(oop: &[f64], ip: &[f64], max_aggressive_actions: u32) -> StreetTree {
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
    StreetTree {
        nlh_rules: street_rules(rules),
        max_aggressive_actions,
        include_allin: false,
        allin_threshold: None,
    }
}
