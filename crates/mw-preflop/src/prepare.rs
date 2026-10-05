//! Preparation and resource validation for the P2 run and view APIs.
use crate::input::{self, Lowered, P2Sections, Settings};
use anyhow::{Result, bail};
use std::path::Path;

/// Parsed common input, normalized artifact text, and typed P2 settings.
pub struct Prepared {
    pub document: spot::Document,
    pub lowered: Lowered,
    pub effective: String,
}

/// Parse, validate, normalize and lower common input into a prepared P2 run.
pub fn prepare(raw: &str, path: &Path) -> Result<Prepared> {
    let document = spot::Document::parse(raw, path)?;
    let settings = Settings::parse(&document.spot, &document.solver, &document.output)?;
    let effective = document.normalize(&P2Sections)?;
    let lowered = input::lower(&document.spot, &settings)?;
    Ok(Prepared {
        document,
        lowered,
        effective,
    })
}

/// Build or restore a production session with a caller-selected cache root; report table readiness.
pub fn build_typed_session(
    lowered: Lowered,
    effective: String,
    checkpoint: Option<&Path>,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(crate::session::AbstractionReady),
) -> Result<crate::session::MultiwaySession> {
    crate::session::build_production_multiway_session(
        lowered, effective, checkpoint, cache_root, on_ready,
    )
}

/// Build or restore a production session from embedded common input.
pub(crate) fn build_session(
    raw: &str,
    checkpoint: Option<&Path>,
    cache_root: Option<&Path>,
    on_ready: &mut dyn FnMut(crate::session::AbstractionReady),
) -> Result<crate::session::MultiwaySession> {
    let p = prepare(raw, Path::new("embedded.toml"))?;
    build_typed_session(p.lowered, p.effective, checkpoint, cache_root, on_ready)
}

/// Count resources and tree rule hits without allocating the policy arena.
pub fn resources(p: &Prepared) -> Result<(crate::session::MultiwayResourcePreflight, Vec<bool>)> {
    crate::session::preflight_multiway_with_rule_hits(p.lowered.clone())
}

/// Reject a prepared run whose public tree exceeds its resource budget.
pub fn check_resources(p: &Prepared) -> Result<()> {
    let (r, _) = resources(p)?;
    ensure_resources(&r)
}

/// Reject an incomplete resource measurement with the matching input diagnostic.
pub fn ensure_resources(r: &crate::session::MultiwayResourcePreflight) -> Result<()> {
    if !r.complete {
        return Err(spot::SpotError::new(
            spot::Code::NLH003,
            "run.memory",
            match r.limit {
                Some(crate::session::ResourceLimit::Node) => {
                    "public tree/policy arena exceeds the node limit"
                }
                _ => "public tree/policy arena exceeds the memory budget",
            },
        )
        .into());
    }
    Ok(())
}

/// Return the display unit for the prepared run utility.
pub fn utility_unit(p: &Prepared) -> &'static str {
    if matches!(p.lowered.utility, crate::UtilityConfig::ChipEv) {
        "BB"
    } else {
        "prizes"
    }
}

// Test oracle: the previous separate walk. Cache exact
// states in a bounded table: collisions cause a revisit, never a false match.
#[cfg(test)]
fn oracle_rule_hits(p: &Prepared) -> Result<Vec<bool>> {
    use crate::betting::BettingMenu;
    use std::hash::{DefaultHasher, Hash, Hasher};
    let rules = &p.document.spot.tree.compiled.rules;
    let mut hits = vec![false; rules.len()];
    let game = p.lowered.game.validated()?;
    let root = crate::BettingState::from_config(&game)?;
    let mut seen: Vec<Option<Vec<u8>>> = vec![None; 1 << 16];
    fn visit(
        state: crate::BettingState,
        betting: &crate::BettingConfig,
        rules: &[nlh::script::Rule<spot::TreeVar>],
        hits: &mut [bool],
        seen: &mut [Option<Vec<u8>>],
        depth: usize,
    ) -> Result<()> {
        if hits.iter().all(|hit| *hit) || state.phase.is_terminal() {
            return Ok(());
        }
        if depth > 512 {
            bail!("NLH003: tree: exceeds the P2 traversal depth limit");
        }
        let key = serde_json::to_vec(&state)?;
        let mut hash = DefaultHasher::new();
        key.hash(&mut hash);
        let slot = hash.finish() as usize & (seen.len() - 1);
        if seen[slot].as_ref() == Some(&key) {
            return Ok(());
        }
        seen[slot] = Some(key);
        let actor = state.to_act.expect("decision actor");
        for (rule, hit) in rules.iter().zip(hits.iter_mut()) {
            if rule.street == state.street
                && rule.condition.eval(&crate::tree_rules::NlhContext {
                    state: &state,
                    actor,
                })
            {
                *hit = true;
            }
        }
        let actions = state
            .legal_actions(betting)
            .map_err(|e| spot::SpotError::new(spot::Code::NLH003, "tree", e.to_string()))?;
        for action in &actions {
            let mut next = state.clone();
            next.apply_from_actions(action.clone(), &actions, betting)?;
            visit(next, betting, rules, hits, seen, depth + 1)?;
        }
        Ok(())
    }
    visit(root, &game.betting, rules, &mut hits, &mut seen, 0)?;
    Ok(hits)
}

/// Return unmatched tree rule warnings in source order.
pub fn warnings_for_hits(game: &crate::MultiwayConfig, hits: &[bool]) -> Vec<String> {
    game.betting
        .nlh_rules
        .iter()
        .zip(hits)
        .enumerate()
        .filter(|(_, (_, hit))| !**hit)
        .map(|(i, (rule, _))| format!("unmatched {:?} tree rule {}", rule.street, i + 1))
        .collect()
}

/// Reject embedded input that requires a removed family before artifact computation.
pub(crate) fn require_artifact_config(raw: &str) -> Result<()> {
    let value: toml::Value = toml::from_str(raw)?;
    let schema = value
        .get("schema")
        .and_then(toml::Value::as_str)
        .unwrap_or("missing schema");
    if schema != "solvers.nlh/v1" {
        bail!(
            "removed config family {schema}: re-solve from a solvers.nlh/v1 config (docs/nlh-input-v1.jp.md)"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FeatureHashAbstraction, HoldemGame};

    fn compare_hits(raw: &str, path: &Path) -> Vec<bool> {
        let p = prepare(raw, path).unwrap();
        let expected = oracle_rule_hits(&p).unwrap();
        let (count, hits) = resources(&p).unwrap();
        assert!(count.complete);
        assert_eq!(hits, expected, "count: {}", path.display());
        assert_eq!(
            count,
            crate::session::preflight_multiway_typed(p.lowered.clone()).unwrap()
        );
        for threads in [1, 4] {
            let game = HoldemGame::new(
                &p.lowered.game,
                &p.lowered.utility,
                &p.lowered.rake,
                FeatureHashAbstraction::default(),
            )
            .unwrap()
            .with_tree_rule_hits();
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .build()
                .unwrap();
            let tree = pool
                .install(|| {
                    crate::tree::enumerate_tree_with_limits_parallel(
                        &game,
                        count.decision_nodes as usize,
                        512,
                    )
                })
                .unwrap();
            assert_eq!(tree.nodes.len() as u64, count.decision_nodes);
            assert_eq!(
                game.tree_rule_hits().unwrap(),
                expected,
                "build ({threads} threads): {}",
                path.display()
            );
        }
        expected
    }

    #[test]
    fn counted_and_built_hits_match_previous_walk_on_smoke_examples() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for name in [
            "3max_2bb",
            "preflop_multiway_v1_production_smoke",
            "preflop_multiway_v1_3max_smoke",
            "6max_2bb",
            "6max_20bb_checkdown",
        ] {
            let directory = if name.starts_with("preflop_multiway_v1_") {
                "crates/mw-preflop/tests/fixtures"
            } else {
                "examples/bench"
            };
            let path = root.join(format!("{directory}/{name}.toml"));
            compare_hits(&std::fs::read_to_string(&path).unwrap(), &path);
        }
    }

    #[test]
    fn hits_count_conditions_even_without_menu_changes_and_skip_checkdown_streets() {
        let raw = "schema = 'solvers.nlh/v1'\n[table]\nplayers = 3\nstack_bb = 2\n[tree]\nscript = '''\npreflop when players == 3 { remove bet }\npreflop when players == 9 { add raise [a] }\nflop, turn, river { checkdown }\n'''\n";
        assert_eq!(
            compare_hits(raw, Path::new("checkdown.toml")),
            [true, false, false, false, false]
        );
        compare_hits(
            "schema = 'solvers.nlh/v1'\n[table]\nplayers = 2\nstack_bb = 2\n",
            Path::new("no-rules.toml"),
        );
    }

    #[test]
    fn node_limited_measurement_retains_only_partial_hits() {
        let raw = "schema = 'solvers.nlh/v1'\n[table]\nplayers = 3\nstack_bb = 2\n[tree]\nscript = 'preflop when position == BB { remove bet }'\n";
        let p = prepare(raw, Path::new("limited.toml")).unwrap();
        let game = HoldemGame::new(
            &p.lowered.game,
            &p.lowered.utility,
            &p.lowered.rake,
            FeatureHashAbstraction::default(),
        )
        .unwrap()
        .with_tree_rule_hits();
        assert!(matches!(
            crate::tree::preflight_arena_with_limits(&game, 1, u64::MAX),
            Err(crate::TreeError::TooManyNodes { limit: 1 })
        ));
        assert_eq!(game.tree_rule_hits().unwrap(), [false]);
        crate::tree::preflight_arena(&game, u64::MAX).unwrap();
        assert_eq!(game.tree_rule_hits().unwrap(), [true]);
    }
    #[test]
    fn resource_refusals_name_the_actual_limit() {
        use crate::session::{MultiwayResourcePreflight, ResourceLimit};
        for (limit, expected) in [
            (ResourceLimit::Node, "node limit"),
            (ResourceLimit::Memory, "memory budget"),
        ] {
            let r = MultiwayResourcePreflight {
                complete: false,
                limit: Some(limit),
                recall: crate::RecallMode::Street,
                decision_nodes: 1,
                terminal_edges: None,
                icm: None,
                policy_columns: None,
                policy_slots: None,
                solver_state_bytes: None,
            };
            let error = ensure_resources(&r).unwrap_err();
            assert!(error.to_string().contains(expected));
            assert_eq!(
                error.downcast_ref::<spot::SpotError>().unwrap().code,
                spot::Code::NLH003
            );
        }
    }
}
