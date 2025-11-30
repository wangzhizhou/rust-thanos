use crate::mca::reader::McaReader;
use crate::mca::writer::McaWriter;
use crate::patterns::inhabited::InhabitedTimePattern;
use crate::patterns::list::ListPattern;
use crate::patterns::ChunkPattern;
use anyhow::{anyhow, Result};
use clap::ValueEnum;
use console::Term;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use log::{info, warn};
use rayon::prelude::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

fn is_dimension_dir(path: &Path) -> bool {
    path.join("region").is_dir()
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(md) = path.metadata() {
        if md.is_file() {
            return md.len();
        }
    }
    if let Ok(rd) = fs::read_dir(path) {
        for ent in rd.flatten() {
            total = total.saturating_add(dir_size(&ent.path()));
        }
    }
    total
}

fn fmt_bytes(mut n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut i = 0;
    let mut val = n as f64;
    while n >= 1024 && i < UNITS.len() - 1 {
        val /= 1024.0;
        n /= 1024;
        i += 1;
    }
    format!("{:.2} {}", val, UNITS[i])
}
fn parse_force_loaded(dimension: &Path) -> Vec<(i32, i32)> {
    let f = dimension.join("data").join("chunks.dat");
    if !f.is_file() {
        return Vec::new();
    }
    let data = match fs::read(&f) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut dec = flate2::read::GzDecoder::new(&data[..]);
    let mut out = Vec::new();
    if std::io::copy(&mut dec, &mut out).is_err() {
        return Vec::new();
    }
    let v = match fastnbt::from_bytes::<fastnbt::Value>(&out) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    match v {
        fastnbt::Value::Compound(m) => {
            let data_tag = match m.get("data") {
                Some(v) => v,
                None => return Vec::new(),
            };
            let mut coords = Vec::new();
            if let fastnbt::Value::Compound(dm) = data_tag {
                // Legacy Forge format: LongArray("Forced"), treat as pairs
                if let Some(fastnbt::Value::LongArray(arr)) = dm.get("Forced") {
                    let mut it = arr.iter();
                    while let (Some(a), Some(b)) = (it.next(), it.next()) {
                        coords.push((*a as i32, *b as i32));
                    }
                }
                // Modern tickets list
                if let Some(fastnbt::Value::List(list)) = dm.get("tickets") {
                    for t in list {
                        if let fastnbt::Value::Compound(tm) = t {
                            if let Some(fastnbt::Value::String(s)) = tm.get("type") {
                                if s == "minecraft:forced" {
                                    if let Some(fastnbt::Value::IntArray(pos)) = tm.get("chunk_pos")
                                    {
                                        if pos.len() == 2 {
                                            coords.push((pos[0], pos[1]));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            coords
        }
        _ => Vec::new(),
    }
}

fn is_valid_mca(path: &Path) -> bool {
    match path.metadata() {
        Ok(meta) => {
            let ok = meta.len() >= 8192;
            if !ok {
                warn!(
                    "Skipping tiny MCA file: {} ({} bytes)",
                    path.display(),
                    meta.len()
                );
            }
            ok
        }
        Err(e) => {
            warn!("Failed to stat MCA file {}: {}", path.display(), e);
            false
        }
    }
}

fn count_total_regions(dims: &[PathBuf]) -> u64 {
    let mut total: u64 = 0;
    for dim in dims {
        let region_dir = dim.join("region");
        if !region_dir.is_dir() {
            continue;
        }
        if let Ok(rd) = fs::read_dir(&region_dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.extension().and_then(|s| s.to_str()) == Some("mca") && is_valid_mca(&p) {
                    total += 1;
                }
            }
        }
    }
    total
}

fn count_total_chunks(dims: &[PathBuf]) -> u64 {
    let mut total: u64 = 0;
    for dim in dims {
        let region_dir = dim.join("region");
        if !region_dir.is_dir() {
            continue;
        }
        if let Ok(rd) = fs::read_dir(&region_dir) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.extension().and_then(|s| s.to_str()) == Some("mca") && is_valid_mca(&p) {
                    if let Ok(mut r) = McaReader::open(p.to_string_lossy().as_ref()) {
                        if let Ok(entries) = r.entries() {
                            total += entries.len() as u64;
                        }
                    }
                }
            }
        }
    }
    total
}

pub fn run(
    input: PathBuf,
    output: Option<PathBuf>,
    inhabited_threshold: i64,
    remove_unknown: bool,
    progress_mode: ProgressMode,
) -> Result<()> {
    if !input.is_dir() {
        return Err(anyhow!("input must be directory"));
    }
    let start_time = std::time::Instant::now();
    let before_size = dir_size(&input);
    let out = output
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join(format!("thanos-{}", uuid::Uuid::new_v4())));
    if out.exists() {
        if out.read_dir()?.next().is_some() {
            return Err(anyhow!("output must be empty"));
        }
    } else {
        fs::create_dir_all(&out)?;
    }
    let mut tasks = Vec::new();
    for entry in fs::read_dir(&input)? {
        let p = entry?.path();
        if p.is_dir() && is_dimension_dir(&p) {
            tasks.push(p);
        }
    }
    if is_dimension_dir(&input) {
        tasks.push(input.clone());
    }
    let _total_regions = count_total_regions(&tasks);
    let total_chunks = count_total_chunks(&tasks);
    let processed_regions = Arc::new(AtomicU64::new(0));
    let processed_chunks = Arc::new(AtomicU64::new(0));
    let removed_total = Arc::new(AtomicU64::new(0));
    let last_pct = Arc::new(AtomicUsize::new(0));

    let mp = Arc::new(MultiProgress::new());
    let term = Term::stdout();
    let is_tty = term.is_term();
    let global_enabled = progress_mode != ProgressMode::Off && is_tty;
    let _region_enabled = progress_mode == ProgressMode::Region && is_tty;
    let global_pb = if global_enabled {
        let (_, cols) = term.size();
        let reserve = 40u16; // spinner + numbers + percent + msg space
        let bar_width = if cols > reserve {
            (cols - reserve) as usize
        } else {
            20usize
        };
        let pb = mp.add(ProgressBar::new(total_chunks.max(1)));
        let style = ProgressStyle::with_template(&format!("{{spinner:.green}} {{bar:{width}.cyan/blue}} {{pos}}/{{len}} 区块 {{percent}}% {{msg}}", width=bar_width))
            .unwrap()
            .progress_chars("=>-");
        pb.set_style(style);
        Some(pb)
    } else {
        None
    };

    tasks.par_iter().try_for_each(|dim| -> Result<()> {
        let rel = dim.strip_prefix(&input).unwrap_or(dim);
        let target_dim = out.join(rel);
        fs::create_dir_all(&target_dim)?;
        let mut patterns: Vec<Box<dyn ChunkPattern + Send>> = Vec::new();
        let forced = parse_force_loaded(dim);
        patterns.push(Box::new(ListPattern::new(forced)));
        patterns.push(Box::new(InhabitedTimePattern::new(
            inhabited_threshold,
            remove_unknown,
        )));
        let region_dir = dim.join("region");
        let entities_dir = dim.join("entities");
        let poi_dir = dim.join("poi");
        fs::create_dir_all(target_dim.join("region"))?;
        if entities_dir.is_dir() {
            fs::create_dir_all(target_dim.join("entities"))?;
        }
        if poi_dir.is_dir() {
            fs::create_dir_all(target_dim.join("poi"))?;
        }

        for entry in fs::read_dir(&region_dir)? {
            let rf = entry?.path();
            if rf.extension().and_then(|s| s.to_str()) != Some("mca") || !is_valid_mca(&rf) {
                continue;
            }
            let name = rf.file_name().unwrap().to_string_lossy().to_string();

            let mut cr = match McaReader::open(rf.to_string_lossy().as_ref()) {
                Ok(r) => r,
                Err(e) => {
                    warn!("Failed to open region MCA {}: {}", rf.display(), e);
                    continue;
                }
            };
            let mut cw = match McaWriter::open(
                target_dim
                    .join("region")
                    .join(&name)
                    .to_string_lossy()
                    .as_ref(),
            ) {
                Ok(w) => w,
                Err(e) => {
                    warn!("Failed to create output region MCA {}: {}", name, e);
                    continue;
                }
            };

            let efile = entities_dir.join(&name);
            let pfile = poi_dir.join(&name);

            let mut ew = None;
            if efile.is_file() && is_valid_mca(&efile) {
                ew = match McaWriter::open(
                    target_dim
                        .join("entities")
                        .join(&name)
                        .to_string_lossy()
                        .as_ref(),
                ) {
                    Ok(w) => Some(w),
                    Err(e) => {
                        warn!("Failed to create output entities MCA {}: {}", name, e);
                        None
                    }
                };
            }
            let mut pw = None;
            if pfile.is_file() && is_valid_mca(&pfile) {
                pw = match McaWriter::open(
                    target_dim
                        .join("poi")
                        .join(&name)
                        .to_string_lossy()
                        .as_ref(),
                ) {
                    Ok(w) => Some(w),
                    Err(e) => {
                        warn!("Failed to create output poi MCA {}: {}", name, e);
                        None
                    }
                };
            }

            let mut region_entries = match cr.entries() {
                Ok(v) => v,
                Err(e) => {
                    warn!("Failed to read chunk entries in {}: {}", name, e);
                    Vec::new()
                }
            };

            let mut er = None;
            if efile.is_file() && is_valid_mca(&efile) {
                er = match McaReader::open(efile.to_string_lossy().as_ref()) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        warn!("Failed to open entities MCA {}: {}", efile.display(), e);
                        None
                    }
                };
            }
            let mut pr = None;
            if pfile.is_file() && is_valid_mca(&pfile) {
                pr = match McaReader::open(pfile.to_string_lossy().as_ref()) {
                    Ok(r) => Some(r),
                    Err(e) => {
                        warn!("Failed to open poi MCA {}: {}", pfile.display(), e);
                        None
                    }
                };
            }

            let mut removed = 0u64;

            for entry in region_entries.iter_mut() {
                let mut keep = false;
                for p in patterns.iter_mut() {
                    if let Ok(m) = p.matches(entry) {
                        if m {
                            keep = true;
                            break;
                        }
                    } else {
                        warn!(
                            "Pattern evaluation failed on chunk {} in {}",
                            entry.region_index(),
                            name
                        );
                    }
                }
                if keep {
                    if let Err(e) = cw.write_entry(entry) {
                        warn!(
                            "Failed to write chunk entry {} in {}: {}",
                            entry.region_index(),
                            name,
                            e
                        );
                    }
                    if let Some(ref mut erdr) = er {
                        match erdr.get(entry.region_index() as usize) {
                            Ok(Some(mut eentry)) => {
                                if let Some(ref mut w) = ew {
                                    if let Err(e) = w.write_entry(&mut eentry) {
                                        warn!(
                                            "Failed to write entities entry {} in {}: {}",
                                            entry.region_index(),
                                            name,
                                            e
                                        );
                                    }
                                }
                            }
                            Ok(None) => {}
                            Err(e) => warn!(
                                "Failed to read entities entry {} in {}: {}",
                                entry.region_index(),
                                name,
                                e
                            ),
                        }
                    }
                    if let Some(ref mut prdr) = pr {
                        match prdr.get(entry.region_index() as usize) {
                            Ok(Some(mut pentry)) => {
                                if let Some(ref mut w) = pw {
                                    if let Err(e) = w.write_entry(&mut pentry) {
                                        warn!(
                                            "Failed to write poi entry {} in {}: {}",
                                            entry.region_index(),
                                            name,
                                            e
                                        );
                                    }
                                }
                            }
                            Ok(None) => {}
                            Err(e) => warn!(
                                "Failed to read poi entry {} in {}: {}",
                                entry.region_index(),
                                name,
                                e
                            ),
                        }
                    }
                } else {
                    removed += 1;
                    removed_total.fetch_add(1, Ordering::Relaxed);
                }
                let new_chunks = processed_chunks.fetch_add(1, Ordering::Relaxed) + 1;
                if let Some(ref pb) = global_pb {
                    pb.set_position(new_chunks);
                    pb.set_message("");
                } else {
                    let pct = (new_chunks * 100 / total_chunks.max(1)) as usize;
                    let prev = last_pct.load(Ordering::Relaxed);
                    if pct > prev {
                        last_pct.store(pct, Ordering::Relaxed);
                        println!("进度: {}% ({}/{})", pct, new_chunks, total_chunks);
                    }
                }
            }

            cw.finalize()?;
            if let Some(ref mut w) = ew {
                w.finalize()?;
            }
            if let Some(ref mut w) = pw {
                w.finalize()?;
            }
            info!("Region {} processed, removed {} chunks", name, removed);
            let _new = processed_regions.fetch_add(1, Ordering::Relaxed) + 1;
        }

        Ok(())
    })?;
    if let Some(pb) = global_pb {
        let done = processed_chunks.load(Ordering::Relaxed);
        let total = total_chunks.max(1);
        pb.set_position(done.min(total));
        pb.finish_with_message("已完成");
    }
    // Second line: summary
    let done = processed_chunks.load(Ordering::Relaxed);
    let removed = removed_total.load(Ordering::Relaxed);
    let kept = done.saturating_sub(removed);
    println!(
        "保留区块总数: {} · 删除区块总数: {} · 总耗时: {:.2}s",
        kept,
        removed,
        start_time.elapsed().as_secs_f64()
    );
    if output.is_none() {
        for dim in &tasks {
            let rel = dim.strip_prefix(&input).unwrap_or(dim);
            let out_dim = out.join(rel);
            let in_dim = input.join(rel);
            for name in ["region", "entities", "poi"] {
                let src = out_dim.join(name);
                if !src.is_dir() {
                    continue;
                }
                let dst = in_dim.join(name);
                fs::create_dir_all(&dst)?;
                let mut keep: std::collections::HashSet<String> = std::collections::HashSet::new();
                for e in fs::read_dir(&src)? {
                    let p = e?.path();
                    if p.extension().and_then(|s| s.to_str()) == Some("mca") {
                        keep.insert(p.file_name().unwrap().to_string_lossy().to_string());
                    }
                }
                if dst.is_dir() {
                    for e in fs::read_dir(&dst)? {
                        let p = e?.path();
                        if p.extension().and_then(|s| s.to_str()) == Some("mca") {
                            let fname = p.file_name().unwrap().to_string_lossy().to_string();
                            if !keep.contains(&fname) {
                                let _ = fs::remove_file(p);
                            }
                        }
                    }
                }
                for e in fs::read_dir(&src)? {
                    let p = e?.path();
                    if p.extension().and_then(|s| s.to_str()) == Some("mca") {
                        let target = dst.join(p.file_name().unwrap());
                        fs::copy(&p, &target)?;
                    }
                }
            }
        }
        fs::remove_dir_all(&out)?;
        let after_size = dir_size(&input);
        let pct = if before_size > 0 {
            (1.0 - (after_size as f64 / before_size as f64)) * 100.0
        } else {
            0.0
        };
        println!(
            "处理前: {} | 处理后: {} | 缩减: {} ({:.2}%)",
            fmt_bytes(before_size),
            fmt_bytes(after_size),
            fmt_bytes(before_size.saturating_sub(after_size)),
            pct
        );
    } else {
        let after_size = dir_size(&out);
        let pct = if before_size > 0 {
            (1.0 - (after_size as f64 / before_size as f64)) * 100.0
        } else {
            0.0
        };
        println!(
            "处理前: {} | 处理后: {} | 缩减: {} ({:.2}%)",
            fmt_bytes(before_size),
            fmt_bytes(after_size),
            fmt_bytes(before_size.saturating_sub(after_size)),
            pct
        );
    }
    Ok(())
}
#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ProgressMode {
    Off,
    Global,
    Region,
}
