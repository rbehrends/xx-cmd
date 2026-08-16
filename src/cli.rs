use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "xx", version, about = "unpack common archive formats safely")]
pub(crate) struct Cli {
    #[arg(short = 'F', long = "format")]
    pub(crate) format: Option<String>,
    #[arg(long)]
    pub(crate) rm: bool,
    #[arg(short = 'O', long)]
    pub(crate) overwrite: bool,
    #[arg(short = 't', long, default_value_t = 1)]
    pub(crate) threads: usize,
    #[arg(short = 'l', long)]
    pub(crate) list: bool,
    #[arg(short = '0', action = clap::ArgAction::SetTrue)]
    pub(crate) zero: bool,
    #[arg(short = 'v', long)]
    pub(crate) verbose: bool,
    #[arg(short = 'J', long)]
    pub(crate) json: bool,
    #[arg(short = 'p', long)]
    pub(crate) progress: bool,
    #[arg(short = 'o', long)]
    pub(crate) output: Option<PathBuf>,
    pub(crate) archive: PathBuf,
}

pub(crate) fn parse() -> Cli {
    Cli::parse()
}
