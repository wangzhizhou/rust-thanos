use rust_thanos::world::ProgressMode;
use std::path::PathBuf;

#[test]
fn run_with_all_progress_modes() {
    let input = PathBuf::from("tests/Fixtures/world");
    let out_off = PathBuf::from("./rust-out-off");
    let _ = std::fs::remove_dir_all(&out_off);
    rust_thanos::world::run(
        input.clone(),
        Some(out_off.clone()),
        0,
        false,
        ProgressMode::Off,
    )
    .unwrap();
    assert!(out_off.join("region").exists());

    let out_global = PathBuf::from("./rust-out-global");
    let _ = std::fs::remove_dir_all(&out_global);
    rust_thanos::world::run(
        input.clone(),
        Some(out_global.clone()),
        0,
        false,
        ProgressMode::Global,
    )
    .unwrap();
    assert!(out_global.join("region").exists());

    let out_region = PathBuf::from("./rust-out-region");
    let _ = std::fs::remove_dir_all(&out_region);
    rust_thanos::world::run(
        input.clone(),
        Some(out_region.clone()),
        0,
        false,
        ProgressMode::Region,
    )
    .unwrap();
    assert!(out_region.join("region").exists());
}
