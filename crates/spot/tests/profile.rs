use spot::{Code, Document, ProductSections, Spot, SpotError};
use std::path::Path;

const BASE: &str = "schema = 'solvers.nlh/v1'\n[table]\nplayers = 2\nstack = 100\n[spot]\nline = 'BTN r2.5, BB c'\nboard = 'Ks 7h 2d'\n[ranges]\nBTN = 'AA'\nBB = 'KK'\n";
struct Hook;
impl ProductSections for Hook {
    fn normalize(
        &self,
        _: &Spot,
        _: &toml::Table,
        _: &toml::Table,
    ) -> Result<(toml_edit::Table, toml_edit::Table), SpotError> {
        Ok((toml_edit::Table::new(), toml_edit::Table::new()))
    }
}
#[test]
fn full_profile_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("base.sol"), b"solution").unwrap();
    let path = dir.path().join("config.toml");
    let text = format!(
        "{BASE}\n[profile]\nfreeze_upstream = {{ solution = 'base.sol' }}\n[[profile.lock]]\nnode = 'x'\nhands = 'AA,AKs'\nstrategy = {{ check = 0.5 }}\n[[profile.lock]]\nnode = 'x [?]'\ncombos = {{ AsKs = {{ check = 1.0 }} }}\nconfidence = 0.5\n[[profile.incentive]]\nnode = 'x'\ncategory = 'toppair'\naction = 'check'\nbb = -0.5\n[[profile.frequency]]\nnode = 'x'\naction = 'check'\ntarget = 0.5\n"
    );
    let first = Document::parse(&text, &path).unwrap();
    let normalized = first.normalize(&Hook).unwrap();
    let second = Document::parse(&normalized, &path).unwrap();
    assert_eq!(first.spot.profile, second.spot.profile);
    assert_eq!(normalized, second.normalize(&Hook).unwrap());
    assert_eq!(
        first
            .spot
            .profile
            .unwrap()
            .freeze_upstream
            .unwrap()
            .sha256
            .unwrap()
            .len(),
        64
    );
    assert!(Document::parse(BASE, &path).unwrap().spot.profile.is_none());
    let empty = Document::parse(&format!("{BASE}\n[profile]\n"), &path).unwrap();
    let effective = empty.normalize(&Hook).unwrap();
    assert_eq!(
        empty.spot.profile,
        Document::parse(&effective, &path).unwrap().spot.profile
    );
    std::fs::write(dir.path().join("base.sol"), b"changed").unwrap();
    let error = match Document::parse(&normalized, &path) {
        Ok(_) => panic!("accepted changed solution"),
        Err(error) => error,
    };
    assert_eq!(error.code, Code::NLH003);
    assert_eq!(error.key.as_deref(), Some("profile.freeze_upstream.sha256"));
}

#[test]
fn embedded_freeze_does_not_require_base_and_hash_is_case_insensitive() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.sol");
    std::fs::write(&base, b"solution").unwrap();
    let path = dir.path().join("config.toml");
    let text = format!("{BASE}\n[profile]\nfreeze_upstream = {{ solution = 'base.sol' }}\n");
    let doc = Document::parse(&text, &path).unwrap();
    let normalized = doc.normalize(&Hook).unwrap();
    let hash = doc
        .spot
        .profile
        .as_ref()
        .unwrap()
        .freeze_upstream
        .as_ref()
        .unwrap()
        .sha256
        .as_ref()
        .unwrap();
    assert!(Document::parse(&normalized.replace(hash, &hash.to_ascii_uppercase()), &path).is_ok());
    std::fs::remove_file(base).unwrap();
    assert!(Document::parse(&normalized, &path).is_err());
    let embedded = Document::parse_embedded(&normalized, &path).unwrap();
    assert_eq!(doc.spot.profile, embedded.spot.profile);
}
#[test]
fn invalid_profiles_are_diagnosed() {
    let cases = [
        (
            "[[profile.lock]]\nstrategy = { check = 1 }",
            "required string",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = ''\nstrategy = { check = 1 }",
            "must not be empty",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nhands = 'AA'\ncategory = 'x'\nstrategy = { check = 1 }",
            "mutually exclusive",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nhands = 'AA:0.5'\nstrategy = { check = 1 }",
            "weights must be exactly 1",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nhands = 'AA:0,KK'\nstrategy = { check = 1 }",
            "weights must be exactly 1",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nstrategy = { check = 1.1 }",
            "[0,1]",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nstrategy = { check = 0.7, call = 0.7 }",
            "sum to at most 1",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nstrategy = { check = 0.5 }\nconfidence = 0.5",
            "sum to 1",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nstrategy = { check = 1 }\nconfidence = 0",
            "(0,1]",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\ncombos = { AsAs = { check = 1 } }",
            "two distinct cards",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nstrategy = {}\ncombos = {}",
            "exactly one",
            Code::NLH003,
        ),
        (
            "[[profile.incentive]]\nnode = 'x'\naction = 'check'\npot = 1\nbb = 1",
            "exactly one",
            Code::NLH003,
        ),
        (
            "[[profile.frequency]]\nnode = 'x'\naction = 'check'\ntarget = -1",
            "[0,1]",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nhands = 'notarange'\nstrategy = { check = 1 }",
            "notarange",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nhands = 'AA'\ncombos = { AsKs = { check = 1 } }",
            "mutually exclusive",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nstrategy = { '' = 1 }",
            "action must not be empty",
            Code::NLH003,
        ),
        (
            "[[profile.incentive]]\nnode = 'x'\naction = 'check'\npot = inf",
            "finite",
            Code::NLH003,
        ),
        (
            "[[profile.incentive]]\nnode = 'x'\naction = ''\nbb = 1",
            "must not be empty",
            Code::NLH003,
        ),
        (
            "[[profile.frequency]]\nnode = 'x'\naction = 'check'",
            "required number",
            Code::NLH003,
        ),
        (
            "[[profile.lock]]\nnode = 'x'\nstrategy = { check = 'one' }",
            "expected number",
            Code::NLH002,
        ),
        ("[profile]\nunknown = 1", "unknown key", Code::NLH002),
    ];
    for (profile, message, code) in cases {
        let e = match Document::parse(&format!("{BASE}\n{profile}\n"), Path::new("config.toml")) {
            Ok(_) => panic!("accepted {profile}"),
            Err(e) => e,
        };
        assert_eq!(e.code, code, "{profile}");
        assert!(e.message.contains(message), "{profile}: {e}");
    }
    let e = match Document::parse(
        "schema = 'solvers.nlh/v1'\n[table]\nplayers = 2\nstack = 100\n[profile]\n",
        Path::new("config.toml"),
    ) {
        Ok(_) => panic!("accepted P2"),
        Err(e) => e,
    };
    assert_eq!(e.code, Code::NLH002);
    assert!(e.message.contains("other product"));
}
