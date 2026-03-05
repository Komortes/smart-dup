mod cli;
mod commands;
mod core;
mod error;
mod output;

use clap::Parser;
use cli::{Cli, Commands};
use error::{AppError, AppResult};

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(err.exit_code());
    }
}

fn run() -> AppResult<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan(args) => commands::scan::run(args).map_err(AppError::runtime_err),
        Commands::Delete(args) => commands::delete::run(args),
        Commands::Photos(args) => commands::photos::run(args).map_err(AppError::runtime_err),
    }
}
