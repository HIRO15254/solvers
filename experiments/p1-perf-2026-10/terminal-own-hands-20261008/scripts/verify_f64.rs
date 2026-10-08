//! Compare decoded solve artifacts bitwise; exclude informational wall time.
use std::path::Path;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let old = Path::new(&args[1]);
    let new = Path::new(&args[2]);
    let mut a = hu_postflop::sol::read_sol(&old.join("solution.sol")).unwrap();
    let mut b = hu_postflop::sol::read_sol(&new.join("solution.sol")).unwrap();
    a.meta.wall_secs = 0.0;
    b.meta.wall_secs = 0.0;
    let a_sol = postcard::to_allocvec(&a).unwrap();
    let b_sol = postcard::to_allocvec(&b).unwrap();
    assert!(a_sol == b_sol, "decoded .sol payload differs");
    let a = hu_postflop::checkpoint::read_checkpoint(&old.join("checkpoint.ckpt")).unwrap();
    let b = hu_postflop::checkpoint::read_checkpoint(&new.join("checkpoint.ckpt")).unwrap();
    assert_eq!(a.config_hash, b.config_hash);
    assert_eq!(a.iteration, b.iteration);
    assert_eq!(a.config_toml, b.config_toml);
    let a_state = postcard::to_allocvec(&a.state).unwrap();
    let b_state = postcard::to_allocvec(&b.state).unwrap();
    assert!(a_state == b_state, "checkpoint state/arena bits differ");
    std::fs::write(new.join("canonical-sol-payload.bin"), &b_sol).unwrap();
    std::fs::write(new.join("canonical-checkpoint-state.bin"), &b_state).unwrap();
    println!(
        "bit-identical: .sol payload {} bytes; checkpoint state {} bytes; iteration={}",
        b_sol.len(),
        b_state.len(),
        b.iteration
    );
}
