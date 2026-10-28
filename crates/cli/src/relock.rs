//! Re-solve a P1 artifact with edited profile constraints.
use anyhow::Result;
use std::io::Read;
use std::path::PathBuf;

#[derive(Debug)]
pub struct InputError(String);
impl std::fmt::Display for InputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for InputError {}
fn input(message: impl std::fmt::Display) -> anyhow::Error {
    InputError(message.to_string()).into()
}

pub struct Args {
    pub source: PathBuf,
    pub out: Option<PathBuf>,
    pub print_config: bool,
    pub add: Vec<PathBuf>,
    pub node: Option<String>,
    pub hands: Option<String>,
    pub category: Option<String>,
    pub strategy: Option<String>,
    pub confidence: Option<f64>,
    pub replace: bool,
    pub clear_profile: bool,
    pub freeze_upstream: bool,
}

pub fn run(args: Args) -> Result<()> {
    let solution = if args.source.is_dir() {
        args.source.join("solution.sol")
    } else {
        args.source.clone()
    };
    if solution.extension().is_none_or(|ext| ext != "sol") {
        return Err(input("source must be a P1 run directory or .sol"));
    }
    let solution = solution.canonicalize().map_err(input)?;
    // Always read the solution, including for run directories: a source must be solved.
    let embedded = hu_postflop::sol::read_sol_config(&solution).map_err(input)?;
    let source = if args.source.is_dir() {
        std::fs::read_to_string(args.source.join("run.toml")).map_err(input)?
    } else {
        embedded
    };
    let mut profile = toml::map::Map::new();
    let mut stdin_used = false;
    for path in &args.add {
        let raw = if path.as_os_str() == "-" {
            if stdin_used {
                return Err(input("stdin may only be used once"));
            }
            stdin_used = true;
            let mut raw = String::new();
            std::io::stdin().read_to_string(&mut raw).map_err(input)?;
            raw
        } else {
            std::fs::read_to_string(path).map_err(input)?
        };
        let mut fragment: toml::Table = toml::from_str(&raw).map_err(input)?;
        let entries = fragment
            .remove("profile")
            .and_then(|v| v.as_table().cloned())
            .ok_or_else(|| input("fragment requires profile arrays"))?;
        if !fragment.is_empty() {
            return Err(input("fragment may contain only profile entries"));
        }
        for (kind, value) in entries {
            if !matches!(kind.as_str(), "lock" | "incentive" | "frequency") {
                return Err(input("fragment may contain only profile entry arrays"));
            }
            let rows = value
                .as_array()
                .ok_or_else(|| input("profile entry must be an array"))?;
            profile
                .entry(kind)
                .or_insert_with(|| toml::Value::Array(Vec::new()))
                .as_array_mut()
                .unwrap()
                .extend(rows.iter().cloned());
        }
    }
    if let Some(node) = args.node {
        let mut row = toml::map::Map::new();
        row.insert("node".into(), node.into());
        if let Some(hands) = args.hands {
            row.insert("hands".into(), hands.into());
        }
        if let Some(category) = args.category {
            row.insert("category".into(), category.into());
        }
        if let Some(confidence) = args.confidence {
            row.insert("confidence".into(), toml::Value::Float(confidence));
        }
        let mut strategy = toml::map::Map::new();
        for pair in args.strategy.as_deref().unwrap_or("").split(',') {
            let (label, probability) = pair
                .split_once('=')
                .ok_or_else(|| input("strategy must contain action=probability pairs"))?;
            let label = label.trim();
            let probability: f64 = probability.trim().parse().map_err(input)?;
            if label.is_empty()
                || !probability.is_finite()
                || strategy
                    .insert(label.to_owned(), toml::Value::Float(probability))
                    .is_some()
            {
                return Err(input("invalid or duplicate strategy action"));
            }
        }
        row.insert("strategy".into(), toml::Value::Table(strategy));
        profile
            .entry("lock".to_owned())
            .or_insert_with(|| toml::Value::Array(Vec::new()))
            .as_array_mut()
            .unwrap()
            .push(toml::Value::Table(row));
    }
    let fragment = if profile.is_empty() {
        None
    } else {
        let mut root = toml::map::Map::new();
        root.insert("profile".into(), toml::Value::Table(profile));
        Some(toml::to_string(&toml::Value::Table(root)).map_err(input)?)
    };
    let edits = spot::RelockEdits {
        add_profile_toml: fragment,
        replace: args.replace,
        clear_profile: args.clear_profile,
        freeze_upstream: args.freeze_upstream.then_some(solution),
    };
    let config = spot::relock_config(&source, &edits)?;
    if args.print_config {
        let prepared = hu_postflop::prepare::prepare(&config, &args.source)?;
        if prepared.document.spot.profile.is_some() {
            hu_postflop::run::validate_profile(&prepared)?;
        }
        print!("{config}");
        Ok(())
    } else {
        crate::nlh_v1::solve(
            &config,
            &args.source,
            args.out.as_deref().expect("clap requires output"),
            None,
            None,
            None,
        )
    }
}
