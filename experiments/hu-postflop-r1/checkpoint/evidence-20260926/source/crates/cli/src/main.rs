//! Thin binary shim over the `cli` library -- see `lib.rs` for everything
//! else, including the subcommand definitions and dispatch.

fn main() {
    if let Err(error) = cli::install_signal_handler() {
        eprintln!("Error: {error:#}");
        std::process::exit(1);
    }
    match cli::main_impl() {
        Ok(()) => {
            let code = cli::CLI_EXIT_CODE.load(std::sync::atomic::Ordering::SeqCst);
            // Cooperative HU cancellation can return Ok without setting a code.
            // Preserve explicit codes (including resource limit 75); errors below
            // retain their own exit code even when cancellation was requested.
            let code = if code == 0 && cli::CLI_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                130
            } else {
                code
            };
            if code != 0 {
                std::process::exit(code);
            }
        }
        Err(error) => {
            let code = cli::error_exit_code(&error);
            eprintln!("Error: {error:#}");
            std::process::exit(code);
        }
    }
}
