use rust_thanos::world::ProgressMode;
use std::path::PathBuf;

#[test]
fn run_on_fixtures_world() {
    let input = PathBuf::from("tests/Fixtures/world");
    let out = PathBuf::from("./rust-out-test");
    if out.exists() {
        std::fs::remove_dir_all(&out).ok();
    }
    rust_thanos::world::run(input, Some(out.clone()), 0, false, ProgressMode::Off)
        .expect("run world");
    assert!(out.join("region").exists());
}
