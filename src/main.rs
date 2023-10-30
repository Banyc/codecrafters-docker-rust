use anyhow::Result;
use clap::{Parser, Subcommand};
use docker_starter_rust::{exec::ExecArgs, rm::RmArgs, run::RunArgs};

#[derive(Debug, Parser)]
pub struct Cli {
    #[clap(subcommand)]
    sub_command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run(RunArgs),
    Exec(ExecArgs),
    Rm(RmArgs),
}

fn main() -> Result<()> {
    let args = Cli::parse();
    match args.sub_command {
        Command::Run(run) => run.run(),
        Command::Exec(exec) => exec.run(),
        Command::Rm(rm) => rm.run(),
    }
}
