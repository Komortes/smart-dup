mod cli;
mod commands;
mod core;
mod output;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan(args) => commands::scan::run(args),
        Commands::Delete(args) => commands::delete::run(args),
        Commands::Photos(args) => commands::photos::run(args),
    }
}
