use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name="bord", version, about="Bord CLI (pre-alpha)")]
struct Cli {
    #[command(subcommand)]
    cmd: Command
}

#[derive(Subcommand)]
enum Command {
    /// List audio devices (stub)
    Devices,
    /// Run a .bord file (stub)
    Run { file: String },
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Command::Devices => {
            println!("(stub) devices: engine backend not wired yet");
        }
        Command::Run { file } => {
            println!("(stub) running file: {file}");
            // call parser stub just to prove linkage
            let _ = bord_dsl::parse_bord("lang 1.0");
            let mut eng = bord_engine::Engine::new();
            eng.start();
            eng.stop();
        }
    }
}
