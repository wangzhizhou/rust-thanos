use rust_thanos::mca::entry::McaEntry;
use rust_thanos::mca::reader::McaReader;
use rust_thanos::mca::writer::McaWriter;
use rust_thanos::world::ProgressMode;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

fn build_inhabited_entry_file(path: &PathBuf, inhabited: i64) {
    let mut data = Vec::new();
    // Long tag (type=4), name length=14, name="InhabitedTime", value (BE i64)
    data.extend_from_slice(b"\x04\x00\x0eInhabitedTime");
    data.extend_from_slice(&inhabited.to_be_bytes());
    let len = (1 + data.len()) as u32; // header length includes method byte
    let mut header = Vec::new();
    header.extend_from_slice(&len.to_be_bytes());
    header.push(3); // RAW
    header.extend_from_slice(&data);
    fs::write(path, header).unwrap();
}

fn create_input_region_with_one_chunk(dir: &Path, inhabited: i64) -> PathBuf {
    let entry_src = dir.join("chunk-src.bin");
    build_inhabited_entry_file(&entry_src, inhabited);
    let entry_file = std::fs::File::open(&entry_src).unwrap();
    let mut entry = McaEntry::new(
        entry_file,
        0,
        fs::metadata(&entry_src).unwrap().len() as usize,
        0,
        0,
        0,
        0,
    );
    let input_region = dir.join("region");
    fs::create_dir_all(&input_region).unwrap();
    let input_mca = input_region.join("r.0.0.mca");
    let mut writer = McaWriter::open(input_mca.to_string_lossy().as_ref()).unwrap();
    writer.write_entry(&mut entry).unwrap();
    writer.finalize().unwrap();
    input_mca
}

#[test]
fn threshold_keeps_and_removes() {
    let base = std::env::temp_dir().join(format!("rt-threshold-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let input_mca = create_input_region_with_one_chunk(&base, 42);

    // run with threshold below 42 (e.g., 10 ticks): expect chunk kept
    let out1 = base.join("out1");
    rust_thanos::world::run(
        base.clone(),
        Some(out1.clone()),
        10,
        false,
        ProgressMode::Off,
    )
    .unwrap();
    let mut r1 = McaReader::open(
        out1.join("region")
            .join(input_mca.file_name().unwrap())
            .to_string_lossy()
            .as_ref(),
    )
    .unwrap();
    let kept1 = r1.entries().unwrap().len();
    assert!(kept1 > 0);

    // run with threshold above 42 (e.g., 100 ticks): expect chunk removed
    let base2 = std::env::temp_dir().join(format!("rt-threshold-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base2).unwrap();
    let _ = create_input_region_with_one_chunk(&base2, 42);
    let out2 = base2.join("out2");
    rust_thanos::world::run(
        base2.clone(),
        Some(out2.clone()),
        100,
        true,
        ProgressMode::Off,
    )
    .unwrap();
    let mut r2 = McaReader::open(
        out2.join("region")
            .join("r.0.0.mca")
            .to_string_lossy()
            .as_ref(),
    )
    .unwrap();
    let kept2 = r2.entries().unwrap().len();
    assert_eq!(kept2, 0);
}

#[test]
fn threshold_equal_is_kept() {
    let base = std::env::temp_dir().join(format!("rt-threshold-eq-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&base).unwrap();
    let _ = create_input_region_with_one_chunk(&base, 42);
    let out = base.join("out");
    rust_thanos::world::run(
        base.clone(),
        Some(out.clone()),
        42,
        false,
        ProgressMode::Off,
    )
    .unwrap();
    let mut r = McaReader::open(
        out.join("region")
            .join("r.0.0.mca")
            .to_string_lossy()
            .as_ref(),
    )
    .unwrap();
    let kept = r.entries().unwrap().len();
    assert!(kept > 0);
}
