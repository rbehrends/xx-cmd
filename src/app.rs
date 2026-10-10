use crate::cli::{self, Cli};
use crate::commit::{Staging, commit, select_logical_root};
use crate::error::{Result, XxError, exit_code};
use crate::extract::{ExtractionReporter, extract_archive};
use crate::format::{derive_output, resolve_format};
use crate::listing::{list_archive, render_listing};
use crate::model::{ListDelimiter, ListFormat, ListKind, OutputMode, Request};
use std::fs;
use std::num::NonZeroUsize;
use std::path::Path;

pub fn cli_main() -> i32 {
    let cli = cli::parse();
    match execute(cli) {
        Ok(()) => 0,
        Err(error) => {
            let code = exit_code(&error);
            eprintln!("xx: {error}");
            code
        }
    }
}

fn execute(cli: Cli) -> Result<()> {
    if cli.threads == 0 {
        return Err(XxError::Usage(
            "--threads must be a positive integer".into(),
        ));
    }
    if cli.rm && cli.overwrite {
        return Err(XxError::Usage(
            "--rm and --overwrite are mutually exclusive".into(),
        ));
    }
    if !cli.list && (cli.zero || cli.json) {
        return Err(XxError::Usage("-0 and --json require --list".into()));
    }
    if cli.list && (cli.rm || cli.overwrite || cli.output.is_some() || cli.no_container_dir) {
        return Err(XxError::Usage(
            "--rm, --overwrite, --output, and --no-container-dir cannot be used with --list".into(),
        ));
    }
    if cli.no_container_dir && cli.rm {
        return Err(XxError::Usage(
            "--rm cannot be used with --no-container-dir".into(),
        ));
    }
    if cli.zero && cli.json {
        return Err(XxError::Usage("-0 cannot be combined with --json".into()));
    }

    let metadata = fs::metadata(&cli.archive).map_err(|source| XxError::InputIo {
        path: cli.archive.clone(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(XxError::Usage(format!(
            "input is not a regular file: {}",
            cli.archive.display()
        )));
    }

    let format = resolve_format(&cli.archive, cli.format.as_deref())?;
    let request = Request {
        input: cli.archive,
        format,
        output: cli.output,
        mode: if cli.rm {
            OutputMode::Replace
        } else if cli.overwrite || cli.no_container_dir {
            OutputMode::Merge
        } else {
            OutputMode::CreateOnly
        },
        threads: NonZeroUsize::new(cli.threads).expect("validated above"),
        list_kind: if cli.list {
            Some(if cli.verbose {
                ListKind::Verbose
            } else {
                ListKind::Names
            })
        } else {
            None
        },
        list_format: if cli.json {
            ListFormat::Json
        } else {
            ListFormat::Text
        },
        delimiter: if cli.zero {
            ListDelimiter::Nul
        } else {
            ListDelimiter::Line
        },
        progress: cli.progress,
        verbose: cli.verbose,
    };

    if let Some(kind) = request.list_kind {
        let records = list_archive(&request)?;
        render_listing(&records, kind, request.list_format, request.delimiter)?;
        return Ok(());
    }

    let output = request
        .output
        .clone()
        .or_else(|| cli.no_container_dir.then(|| Path::new(".").to_path_buf()))
        .or_else(|| derive_output(&request.input))
        .ok_or_else(|| {
            XxError::Usage(
                "an output path is required when the input suffix is not recognized".into(),
            )
        })?;
    validate_output_parent(&output, &request.input, request.mode)?;
    let staging = Staging::create(output_parent(&output))?;
    let reporter = ExtractionReporter::new(request.verbose, request.progress, request.format);
    reporter.start();
    extract_archive(&request, staging.path(), &reporter)?;
    let logical_root = select_logical_root(staging.path())?;
    let move_source = logical_root.clone();
    commit(staging, logical_root, &output, request.mode)?;
    reporter.move_to(&move_source, &output)?;
    Ok(())
}
pub(crate) fn output_parent(output: &Path) -> &Path {
    output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

pub(crate) fn validate_output_parent(output: &Path, input: &Path, mode: OutputMode) -> Result<()> {
    if output == input {
        return Err(XxError::OutputConflict(
            output.to_path_buf(),
            "output is the input archive".into(),
        ));
    }
    if output.exists()
        && fs::symlink_metadata(output)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    {
        return Err(XxError::OutputConflict(
            output.to_path_buf(),
            "output is a symbolic link".into(),
        ));
    }
    if mode == OutputMode::Replace && output.exists() {
        let canonical_output = fs::canonicalize(output).map_err(|source| XxError::OutputIo {
            path: output.to_path_buf(),
            source,
        })?;
        let input_parent = input
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let canonical_input_parent =
            fs::canonicalize(input_parent).map_err(|source| XxError::InputIo {
                path: input.to_path_buf(),
                source,
            })?;
        let input_name = input.file_name().ok_or_else(|| {
            XxError::OutputConflict(
                output.to_path_buf(),
                "input archive path has no file name".into(),
            )
        })?;
        let canonical_input = canonical_input_parent.join(input_name);
        if canonical_input.starts_with(&canonical_output) {
            return Err(XxError::OutputConflict(
                output.to_path_buf(),
                "output contains the input archive".into(),
            ));
        }
    }
    let parent = output_parent(output);
    let metadata = fs::metadata(parent).map_err(|source| XxError::OutputIo {
        path: parent.to_path_buf(),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(XxError::OutputConflict(
            parent.to_path_buf(),
            "output parent is not a directory".into(),
        ));
    }
    Ok(())
}
