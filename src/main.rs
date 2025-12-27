use crate::world::ProgressMode;
use anyhow::Result;
use chrono::Local;
use clap::Parser;
use console::Term;
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;
use std::path::PathBuf;
use walkdir::WalkDir;
use zip::write::FileOptions;

mod mca;
mod patterns;
mod world;

#[derive(Parser)]
#[command(
    name = "rust-thanos",
    version,
    about = "Optimize Minecraft Java worlds",
    long_about = "Scan MCA region files and remove unused chunks. Keep chunks by InhabitedTime threshold and force-loaded tickets. Supports RAW/ZLIB/GZIP/LZ4. Recursively detects dimension directories. Friendly progress display. Optional zip archive of OUTPUT_DIR.",
    after_help = "Examples:\n  rust-thanos /world /out\n  rust-thanos /world /out -t 600\n  rust-thanos /world /out --remove-unknown --progress-mode global\n  rust-thanos /world --in-place\n  rust-thanos /world /out --zip-output\n  rust-thanos /world /out -f\n\nNotes:\n  • InhabitedTime threshold unit: seconds (1s = 20 ticks).\n  • Non in-place mode requires OUTPUT_DIR to be empty; if it exists and is not empty, you will be prompted. Use -f/--force to overwrite without prompt.\n  • If OUTPUT_DIR is omitted, a temporary directory is used and replaces the input directory.\n  • On Windows, WSL is recommended for better performance."
)]
struct Args {
    #[arg(value_name = "WORLD_DIR", help = "Minecraft world root")]
    input: PathBuf,
    #[arg(
        value_name = "OUTPUT_DIR",
        help = "Output directory (must be empty; optional)"
    )]
    output: Option<PathBuf>,
    #[arg(
        long = "inhabited-time-seconds",
        short = 't',
        default_value_t = 300,
        help = "InhabitedTime threshold in seconds (1s = 20 ticks)"
    )]
    inhabited_time_seconds: i64,
    #[arg(
        long,
        default_value_t = false,
        help = "Treat unknown/external-compressed chunks as removable"
    )]
    remove_unknown: bool,
    #[arg(long, value_enum, default_value_t = ProgressMode::Region, help = "Progress display: off | global | region")]
    progress_mode: ProgressMode,
    #[arg(
        long,
        default_value_t = false,
        help = "Process in-place: ignore OUTPUT_DIR and replace WORLD_DIR"
    )]
    in_place: bool,
    #[arg(
        long,
        default_value_t = false,
        help = "Zip OUTPUT_DIR to timestamped archive (YYYYMMddHHmmss.zip) and remove it"
    )]
    zip_output: bool,
    #[arg(
        short = 'f',
        long,
        default_value_t = false,
        help = "Force overwrite OUTPUT_DIR if it exists (no prompt)"
    )]
    force: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    let ticks = args
        .inhabited_time_seconds
        .checked_mul(20)
        .ok_or_else(|| anyhow::anyhow!("inhabited threshold seconds overflow"))?;
    if !args.in_place {
        if let Some(ref out_dir) = args.output {
            if out_dir.exists() {
                let non_empty = out_dir.read_dir()?.next().is_some();
                if non_empty {
                    if args.force {
                        std::fs::remove_dir_all(out_dir)?;
                    } else {
                        print!("输出目录已存在，是否覆盖？[y/N]: ");
                        std::io::stdout().flush().ok();
                        let mut line = String::new();
                        std::io::stdin().read_line(&mut line)?;
                        let resp = line.trim().to_lowercase();
                        if resp == "y" || resp == "yes" {
                            std::fs::remove_dir_all(out_dir)?;
                        } else {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
    let dest = if args.in_place {
        None
    } else {
        args.output.clone()
    };
    world::run(
        args.input,
        dest,
        ticks,
        args.remove_unknown,
        args.progress_mode,
    )?;
    if !args.in_place {
        if let Some(ref out_dir) = args.output {
            if args.zip_output {
                let ts = Local::now().format("%Y%m%d%H%M%S").to_string();
                let parent = out_dir.parent().unwrap_or(Path::new("."));
                let zip_path = parent.join(format!("{}.zip", ts));
                println!("开始压缩: {} → {}", out_dir.display(), zip_path.display());
                zip_dir(out_dir, &zip_path)?;
                println!("zip: {}", zip_path.display());
                std::fs::remove_dir_all(out_dir)?;
            }
        }
    }
    Ok(())
}

fn zip_dir(src_dir: &Path, dst_zip: &Path) -> Result<()> {
    let file = File::create(dst_zip)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let total_files: u64 = WalkDir::new(src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .count() as u64;
    let pb = ProgressBar::new(total_files.max(1));
    let term = Term::stdout();
    let (_, cols) = term.size();
    let reserve = 30u16;
    let mut bar_width = if cols > reserve {
        (cols - reserve) as usize
    } else {
        20usize
    };
    bar_width = bar_width.min(50);
    let style = ProgressStyle::with_template(&format!(
        "{{spinner:.green}} {{bar:{width}.cyan/blue}} {{pos}}/{{len}} 文件 {{percent}}% {{msg}}",
        width = bar_width
    ))
    .unwrap()
    .progress_chars("=>-");
    pb.set_style(style);
    for entry in WalkDir::new(src_dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = path.strip_prefix(src_dir).unwrap();
        if name.as_os_str().is_empty() {
            continue;
        }
        if path.is_file() {
            pb.set_message(name.to_string_lossy().to_string());
            zip.start_file(name.to_string_lossy(), options)?;
            let mut f = std::fs::File::open(path)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            zip.write_all(&buf)?;
            pb.inc(1);
        } else {
            let dir_name = format!("{}/", name.to_string_lossy());
            zip.add_directory(dir_name, options)?;
        }
    }
    zip.finish()?;
    pb.finish_with_message("压缩完成");
    Ok(())
}
