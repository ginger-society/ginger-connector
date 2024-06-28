use core::fmt;
use std::path::Path;

use clap::{Parser, Subcommand, ValueEnum};
use init::initialize;
use service::generate_client;
use utils::fetch_metadata_and_process;

mod init;
mod service;
mod utils;
/// Command line interface for managing the application
#[derive(Parser)]
#[clap(name = "CLI")]
#[clap(about = "A CLI for managing service dependencies", long_about = None)]
struct CLI {
    #[clap(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Fetch metadata and process it
    Init {
        #[clap(value_parser)]
        repo_path: String,
    },
    Config,
    /// Connect to an environment
    Connect {
        #[clap(value_enum, default_value_t=Environment::Dev)]
        env: Environment,
    },
}

#[derive(ValueEnum, Clone)]
enum Environment {
    Dev,
    Stage,
    Prod,
}
impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Environment::Dev => write!(f, "dev"),
            Environment::Stage => write!(f, "stage"),
            Environment::Prod => write!(f, "prod"),
        }
    }
}

fn main() {
    let cli = CLI::parse();

    let config_path = Path::new("services.toml");

    match &cli.command {
        Commands::Config {} => {
            fetch_metadata_and_process(config_path);
        }
        Commands::Connect { env } => generate_client(config_path, env.clone()),
        Commands::Init { repo_path } => initialize(repo_path, config_path),
    }
}
