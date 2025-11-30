use rust_thanos::world::ProgressMode;
use std::fs;
use std::path::PathBuf;

#[test]
fn inplace_processing_replaces_world() {
    let src = PathBuf::from("tests/Fixtures/world");
    let tmp = std::env::temp_dir().join(format!("rt-inplace-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&tmp).unwrap();
    let world = tmp.join("world");
    fs::create_dir_all(&world).unwrap();
    let mut opts = fs_extra::dir::CopyOptions::new();
    opts.content_only = true;
    fs_extra::dir::copy(&src, &world, &opts).unwrap();
    rust_thanos::world::run(world.clone(), None, 0, false, ProgressMode::Off).unwrap();
    assert!(world.join("region").exists());
}
