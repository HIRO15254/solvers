//! Product-independent CLI argument values for artifact commands.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum InspectView {
    Node,
    Summary,
    Strategy,
    Range,
    Ev,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ExportView {
    Strategy,
    Actions,
    Range,
    Ev,
    Tree,
    Summary,
    Decision,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ExportFormat {
    Json,
    Csv,
}
