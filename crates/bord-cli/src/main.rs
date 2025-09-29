use clap::{Args, Parser, Subcommand};

#[derive(Parser)]
#[command(name="bord", version, about="Bord CLI (pre-alpha)")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List audio devices
    Devices,
    /// Start audio (passthrough + Gain effect for now)
    Run(RunArgs),
}

#[derive(Args, Debug)]
struct RunArgs {
    /// Input device substring (case-insensitive). If set, overrides --in-idx.
    #[arg(long = "in")]
    in_name: Option<String>,

    /// Output device substring (case-insensitive). If set, overrides --out-idx.
    #[arg(long = "out")]
    out_name: Option<String>,

    /// Input device by index (see `bord devices`). Ignored if --in is set.
    #[arg(long = "in-idx")]
    in_idx: Option<usize>,

    /// Output device by index (see `bord devices`). Ignored if --out is set.
    #[arg(long = "out-idx")]
    out_idx: Option<usize>,

    /// Sample rate (e.g., 48000)
    #[arg(long = "sr")]
    sample_rate: Option<u32>,

    /// Frames per buffer (if supported by backend), e.g., 128 or 256
    #[arg(long = "block")]
    block_size: Option<u32>,

    /// Simple test effect: gain in dB (e.g., -6.0, 0.0, +6.0)
    #[arg(long = "gain-db", default_value_t = 0.0)]
    gain_db: f32,
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Devices => {
            if let Err(e) = bord_engine::devices::print_devices() {
                eprintln!("Error listing devices: {e:?}");
                std::process::exit(1);
            }
        }
        Command::Run(args) => {
            let cfg = bord_engine::EngineConfig {
                input_name: args.in_name,
                output_name: args.out_name,
                input_index: args.in_idx,
                output_index: args.out_idx,
                sample_rate: args.sample_rate,
                block_size: args.block_size,
                gain_db: args.gain_db,
            };
            println!("Starting with config: {cfg:?}");
            let mut eng = bord_engine::Engine::new(cfg);
            if let Err(e) = eng.start() {
                eprintln!("Engine start error: {e:?}");
                std::process::exit(1);
            }
            println!("Audio running. Ctrl+C to stop.");
            loop { std::thread::sleep(std::time::Duration::from_millis(500)); }
        }
    }
}

