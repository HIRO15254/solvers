//! Thin binary shim over the `cli` library -- see `lib.rs` for everything
//! else, including the subcommand definitions and dispatch.

fn main() -> anyhow::Result<()> {
    cli::main_impl()
}
