use crate::world::ProgressMode;
use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;

mod mca;
mod patterns;
mod world;

#[derive(Parser)]
#[command(
    name = "rust-thanos",
    version,
    about = "Detect and remove unused chunks from Minecraft Java worlds",
    long_about = "Rust implementation of Thanos: scans MCA region files and removes chunks not matching keep-patterns (e.g., InhabitedTime, force-loaded). Supports RAW/ZLIB/GZIP/LZ4.",
    after_help = "Examples:\n  rust-thanos /path/to/world /path/to/out\n  rust-thanos /world /out --inhabited-seconds 600\n  rust-thanos /world /out --remove-unknown --progress-mode global\n\nNotes:\n  - Inhabited time threshold is in seconds (1s = 20 ticks).\n  - Output directory must be empty. If omitted, a temp dir is used and replaces input.\n  - For Windows, WSL is recommended for better performance."
)]
struct Args {
    #[arg(
        value_name = "WORLD_DIR",
        help = "Path to the Minecraft world directory"
    )]
    input: PathBuf,
    #[arg(
        value_name = "OUTPUT_DIR",
        help = "Empty directory to write the cleaned world (optional)"
    )]
    output: Option<PathBuf>,
    #[arg(
        long = "inhabited-time-seconds",
        short = 't',
        default_value_t = 300,
        help = "Seconds for inhabited time threshold (1s = 20 ticks)"
    )]
    inhabited_time_seconds: i64,
    #[arg(
        long,
        default_value_t = false,
        help = "Treat unknown/external-compressed chunks as removable"
    )]
    remove_unknown: bool,
    #[arg(long, value_enum, default_value_t = ProgressMode::Region, help = "Progress display mode: off | global | region")]
    progress_mode: ProgressMode,
    #[arg(
        long,
        default_value_t = false,
        help = "Process in-place: ignore OUTPUT_DIR, replace WORLD_DIR with cleaned result"
    )]
    in_place: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    let ticks = args
        .inhabited_time_seconds
        .checked_mul(20)
        .ok_or_else(|| anyhow::anyhow!("inhabited threshold seconds overflow"))?;
    let dest = if args.in_place { None } else { args.output };
    world::run(
        args.input,
        dest,
        ticks,
        args.remove_unknown,
        args.progress_mode,
    )?;
    Ok(())
}
