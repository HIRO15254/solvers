//! Serializable common validation evidence, independent of product algorithms.
use crate::*;
use nlh::SizeUnit;
use nlh::script::{Condition, Effect, ParamKind};

pub(crate) fn document(doc: &Document) -> ValidateSummary {
    let spot = &doc.spot;
    ValidateSummary {
        product: spot.product,
        start: spot.context.clone(),
        effective_stack: spot.context.effective_stack,
        actions: spot.context.actions.clone(),
        tree: TreeDiagnostics {
            params: spot
                .tree
                .compiled
                .params
                .iter()
                .map(|p| ParamDiagnostic {
                    name: p.name.clone(),
                    kind: match p.kind {
                        ParamKind::Number => "number",
                        ParamKind::Bool => "bool",
                        ParamKind::Token => "token",
                    }
                    .into(),
                    value: p.default.clone(),
                    description: p.description.clone(),
                })
                .collect(),
            rules: spot
                .tree
                .compiled
                .rules
                .iter()
                .map(|r| {
                    let effect = match r.effect {
                        Effect::Add => "add",
                        Effect::Remove => "remove",
                        Effect::Replace => "replace",
                        Effect::Force => "force",
                        Effect::Checkdown => "checkdown",
                    };
                    let mut body = effect.to_owned();
                    if let Some(action) = r.action {
                        body.push_str(&format!(" {}", action.name()));
                        if !r.sizes.is_empty() {
                            body.push_str(&format!(
                                " [{}]",
                                r.sizes
                                    .iter()
                                    .map(|s| s.render(SizeUnit::Bb))
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                    }
                    let condition = match &r.condition {
                        Condition::Const(true) => String::new(),
                        Condition::Const(false) => " when unopened && !unopened".into(),
                        condition => format!(" when {condition}"),
                    };
                    format!(
                        "{}{condition} {{ {body} }}",
                        ["preflop", "flop", "turn", "river"][r.street.index()]
                    )
                })
                .collect(),
        },
        // Unmatched-rule and solver-stop warnings require the product's built tree/settings.
        warnings: Vec::new(),
    }
}
