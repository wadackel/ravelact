mod shell_completion;

use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::env::{CompleteEnv, Shells};
use ravelact::cli::Cli;

use shell_completion::{FilteredBash, FilteredFish, FilteredZsh};

fn main() -> Result<()> {
    CompleteEnv::with_factory(Cli::command)
        .shells(Shells(&[&FilteredBash, &FilteredZsh, &FilteredFish]))
        .complete();

    let cli = Cli::parse();
    let code = cli.run()?;
    std::process::exit(code);
}
