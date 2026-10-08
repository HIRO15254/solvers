use std::path::Path;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mut sol = hu_postflop::sol::read_sol(Path::new(&args[1])).unwrap();
    let ckpt = hu_postflop::checkpoint::read_checkpoint(Path::new(&args[2])).unwrap();
    sol.meta.wall_secs = 0.0;
    // Operational thread count differs between thread-invariance runs only.
    sol.config_toml = sol.config_toml.lines().filter(|l| !l.starts_with("threads = ")).collect::<Vec<_>>().join("\n");
    std::fs::write(format!("{}.sol-payload", args[3]), postcard::to_allocvec(&sol).unwrap()).unwrap();
    std::fs::write(format!("{}.checkpoint-state", args[3]), postcard::to_allocvec(&ckpt.state).unwrap()).unwrap();
}
