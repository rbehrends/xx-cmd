use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "xx",
    version,
    about = "Unpack common archive formats safely",
    long_about = "xx extracts common archive formats into a directory derived from the archive name, or into a directory you choose with --output. Use --no-container-dir to extract directly into the current or specified output directory. It recognizes formats by filename suffix and supports tar, PAX, cpio, ar, 7z, ZIP, and JAR archives, including supported compressed tar, PAX, and cpio variants. Use --format when the format cannot be inferred from the filename.\n\nBy default, extraction is staged in a temporary directory beside the destination, validated, and then published. xx rejects unsafe paths and links that escape the extracted tree. Existing output directories are left untouched unless --rm or --overwrite is selected. With --no-container-dir, entries are merged into the current or specified output directory and conflicts are replaced; --overwrite is not required, and --rm cannot be combined with that option.\n\nUse --list to display archive contents instead of extracting them. Listings can include metadata, use NUL-delimited fields, or be emitted as JSON."
)]
pub(crate) struct Cli {
    #[arg(
        short = 'F',
        long = "format",
        help = "Select the archive format (required if it cannot be inferred from the filename)"
    )]
    pub(crate) format: Option<String>,
    #[arg(
        long,
        help = "Remove an existing output directory after successful extraction"
    )]
    pub(crate) rm: bool,
    #[arg(
        short = 'O',
        long,
        help = "Merge into an existing output directory, replacing conflicts"
    )]
    pub(crate) overwrite: bool,
    #[arg(
        short = 't',
        long,
        default_value_t = 1,
        help = "Maximum number of extraction workers"
    )]
    pub(crate) threads: usize,
    #[arg(
        short = 'l',
        long,
        help = "List archive contents instead of extracting"
    )]
    pub(crate) list: bool,
    #[arg(short = '0', action = clap::ArgAction::SetTrue, help = "Terminate listed fields with NUL bytes")]
    pub(crate) zero: bool,
    #[arg(
        short = 'v',
        long,
        help = "Include listing metadata or report extracted files"
    )]
    pub(crate) verbose: bool,
    #[arg(short = 'J', long, help = "Output the listing as JSON")]
    pub(crate) json: bool,
    #[arg(
        short = 'p',
        long,
        help = "Report extraction or listing progress to standard error"
    )]
    pub(crate) progress: bool,
    #[arg(
        short = 'o',
        long,
        help = "Write output to this directory instead of the derived name"
    )]
    pub(crate) output: Option<PathBuf>,
    #[arg(
        short = 'n',
        long = "no-container-dir",
        help = "Merge directly into the current directory or --output directory, replacing conflicts"
    )]
    pub(crate) no_container_dir: bool,
    #[arg(help = "Archive file to list or extract")]
    pub(crate) archive: PathBuf,
}

pub(crate) fn parse() -> Cli {
    Cli::parse()
}
