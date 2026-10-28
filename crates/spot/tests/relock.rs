use spot::{Document, RelockEdits, relock_config};
use std::path::Path;

const BASE: &str = "schema = 'solvers.nlh/v1'\n[table]\nplayers = 2\nstack_bb = 100\n[spot]\nline = 'BTN r2.5, BB c'\nboard = 'Ks 7h 2d'\n[ranges]\nBTN = 'AA'\nBB = 'KK'\n";
const ORIGINAL: &str = "[[profile.lock]]\nnode = 'x'\nhands = 'AA'\nstrategy = { check = 1 }\n[[profile.lock]]\nnode = 'y'\nstrategy = { check = 1 }\n[[profile.incentive]]\nnode = 'x'\naction = 'check'\nbb = 1\n[[profile.frequency]]\nnode = 'x'\naction = 'check'\ntarget = 0.5\n";
const ADD: &str = "[[profile.lock]]\nnode = 'x'\nhands = 'KK'\nstrategy = { check = 1 }\n[[profile.incentive]]\nnode = 'x'\naction = 'check'\nbb = 2\n[[profile.frequency]]\nnode = 'x'\naction = 'check'\ntarget = 0.7\n";

fn profile(text: &str) -> spot::ProfileIr {
    Document::parse(text, Path::new("run.toml"))
        .unwrap()
        .spot
        .profile
        .unwrap()
}

#[test]
fn append_preserves_existing_entries() {
    let result = relock_config(
        &format!("{BASE}{ORIGINAL}"),
        &RelockEdits {
            add_profile_toml: Some(ADD.into()),
            ..Default::default()
        },
    )
    .unwrap();
    let p = profile(&result);
    assert_eq!(p.locks.len(), 3);
    assert_eq!(p.incentives.len(), 2);
    assert_eq!(p.frequencies.len(), 2);
    assert!(
        p.locks
            .iter()
            .any(|lock| lock.node == "x" && lock.hands == spot::HandSelector::Range("KK".into()))
    );
    assert!(
        p.locks
            .iter()
            .any(|lock| lock.node == "x" && lock.hands == spot::HandSelector::Range("AA".into()))
    );
    assert!(p.locks.iter().any(|lock| lock.node == "y"));
    assert_eq!(p.locks[2].node, "x");
    assert_eq!(p.locks[2].hands, spot::HandSelector::Range("KK".into()));
}

#[test]
fn replace_matches_node_and_kind_only() {
    let result = relock_config(
        &format!("{BASE}{ORIGINAL}"),
        &RelockEdits {
            add_profile_toml: Some(ADD.into()),
            replace: true,
            ..Default::default()
        },
    )
    .unwrap();
    let p = profile(&result);
    assert_eq!(p.locks.len(), 2);
    assert!(p.locks.iter().any(|lock| lock.node == "y"));
    assert!(
        p.locks
            .iter()
            .any(|lock| lock.node == "x" && lock.hands == spot::HandSelector::Range("KK".into()))
    );
    assert_eq!(p.locks[0].node, "y");
    assert_eq!(p.locks[1].node, "x");
    assert_eq!(p.incentives.len(), 1);
    assert_eq!(p.frequencies.len(), 1);
    assert_eq!(p.frequencies[0].target, 0.7);
}

#[test]
fn clear_removes_profile_and_prior_freeze() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.sol");
    std::fs::write(&path, b"solution").unwrap();
    let source = relock_config(
        &format!("{BASE}{ORIGINAL}"),
        &RelockEdits {
            freeze_upstream: Some(path),
            ..Default::default()
        },
    )
    .unwrap();
    let result = relock_config(
        &source,
        &RelockEdits {
            clear_profile: true,
            add_profile_toml: Some(ADD.into()),
            ..Default::default()
        },
    )
    .unwrap();
    let p = profile(&result);
    assert!(p.freeze_upstream.is_none());
    assert_eq!(p.locks.len(), 1);
    assert_eq!(p.incentives.len(), 1);
}

#[test]
fn freeze_is_parsed_and_hashed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.sol");
    std::fs::write(&path, b"solution").unwrap();
    let result = relock_config(
        BASE,
        &RelockEdits {
            freeze_upstream: Some(path.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let freeze = profile(&result).freeze_upstream.unwrap();
    assert_eq!(freeze.solution, path.to_str().unwrap());
    assert_eq!(freeze.sha256.unwrap().len(), 64);
}

#[test]
fn rejects_p2_and_invalid_fragments() {
    let p2 = "schema = 'solvers.nlh/v1'\n[table]\nplayers = 6\nstack_bb = 100\n";
    assert!(relock_config(p2, &RelockEdits::default()).is_err());
    for fragment in [
        "[solver]\nx = 1",
        "[profile]\nfreeze_upstream = { solution = 'x' }",
        "[[profile.lock]]\nstrategy = { check = 1 }",
        "[[profile.lock]]\nnode = 'x'\nstrategy = { check = 2 }",
    ] {
        assert!(
            relock_config(
                BASE,
                &RelockEdits {
                    add_profile_toml: Some(fragment.into()),
                    ..Default::default()
                }
            )
            .is_err(),
            "{fragment}"
        );
    }
}
