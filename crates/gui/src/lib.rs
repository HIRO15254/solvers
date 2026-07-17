//! Native egui/eframe GUI for the multiway preflop solver
//! (Setup / Solve / Results). See `docs/native-gui-plan.md` section F.
//!
//! Scope: `kind = "preflop-multiway"` configs only; the HU postflop
//! workbench stays in `web/`. Library form exists so integration tests can
//! drive the solve worker headlessly.

pub mod action_style;
pub mod app;
pub mod eval_table;
pub mod format;
pub mod frequency;
pub mod matrix;
pub mod model;
pub mod presets;
pub mod results;
pub mod setup;
pub mod size_lexer;
pub mod solve_view;
pub mod status;
pub mod theme;
pub mod worker;
