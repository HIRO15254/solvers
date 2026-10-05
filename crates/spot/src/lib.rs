//! Strict `solvers.nlh/v1` document parsing and normalization.
//! Product implementations own solver/output validation through `ProductSections`.
pub mod derive;
mod dialect;
mod error;
mod ir;
mod normalize;
mod parse;
mod replay;
mod summary;

pub use dialect::{NLH_V1, TreeVar, parse_size_literal};
pub use error::{Code, SpotError};
pub use ir::*;
use std::path::Path;

/// Parsed common IR plus uninterpreted product-owned tables.
pub struct Document {
    pub spot: Spot,
    pub solver: toml::Table,
    pub output: toml::Table,
}

/// Product-owned validation returns insertion-ordered tables with every default explicit.
/// Products may serialize typed settings with `toml_edit::ser` to retain field order.
/// The common parser never inspects method-specific solver or output settings.
pub trait ProductSections {
    fn normalize(
        &self,
        spot: &Spot,
        solver: &toml::Table,
        output: &toml::Table,
    ) -> Result<(toml_edit::Table, toml_edit::Table), SpotError>;
}

/// A diagnostic for a setting belonging to the other product.
pub fn other_product_key(spot: &Spot, key: impl Into<String>) -> SpotError {
    SpotError::new(
        Code::NLH002,
        key,
        format!(
            "this spot is solved by {}; this key belongs to the other product",
            spot.product.name()
        ),
    )
}

impl Document {
    /// Resolve tree source paths relative to this config file (no environment expansion).
    pub fn parse(text: &str, config_path: &Path) -> Result<Self, SpotError> {
        parse::document(text, config_path)
    }

    /// Deterministic self-contained TOML; product defaults are supplied by the hook.
    pub fn normalize(&self, product: &impl ProductSections) -> Result<String, SpotError> {
        normalize::document(self, product)
    }

    /// Common validation evidence; products may add their own operating warnings.
    pub fn summary(&self) -> ValidateSummary {
        summary::document(self)
    }
}
