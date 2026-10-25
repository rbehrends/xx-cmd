# xx

`xx` unpacks common archive formats with one command.

```console
$ xx project.tar.gz
```

This command extracts `project.tar.gz` into `project`.

## Features

`xx` extracts tar, pax, cpio, ar, 7z, zip, and jar archives. It recognizes formats based on their extension and also supports tar, PAX, and cpio archives that have been compressed with gzip, bunzip2, xz, zstd, or Unix `compress`.

It can list archive names and metadata as text, NUL-delimited fields, or JSON. During extraction, `xx` builds the result in a temporary staging directory before updating the destination.

It has the dual purpose of (1) not having to remember and type the exact command for extracting a specific archive format and (2) making the output consistently go to a predefined directory rather than spilling it all over the current directory if the archive has no root directory.

## Installation

`xx` uses Rust 2024. Install a current Rust toolchain, clone this repository, and run:

```console
$ cargo install --path . --locked
```

or directly from GitHub:

```console
$ cargo install --git https://github.com/rbehrends/xx-cmd
```

The default build includes 7z support. To omit it, build without default features:

```console
$ cargo install --path . --locked --no-default-features
```

To build without installing, run:

```console
$ cargo build --release --locked
```

Cargo writes the binary to `target/release/xx`.

## Usage

```console
$ xx [OPTIONS] <ARCHIVE>
```

Extract an archive to a directory derived from its name:

```console
$ xx source.tar.xz
```

This creates the directory `source` in the current working directory and populates it with the archive's contents; the archive's parent directory is not used for the derived destination. Note that if all files in the archive are contained inside the same directory, that directory becomes the output directory.

Staging directories are named `.xx-*` and are in the same directory as the output. They are normally deleted upon completion, but interrupting the command may result in them staying around.

Use `-v/--verbose` during extraction to print each directory and regular file as it is written to staging. Directories use a trailing native path separator (`src/` on Unix-like systems and `src\` on Windows) and are printed without a size; regular files are followed by their human-readable size, for example `src/main.rs (12k)`. After validation, the final line shows the staged root being moved to the output directory. These messages are written to standard output.

Choose another destination:

```console
$ xx --output vendor/source source.tar.xz
```

Select the format when the filename does not identify it:

```console
$ xx --format tar-gzip --output source download.data
```

If the filename has no recognized suffix, `--format` selects the decoder, but `--output` is still required.

### Output directories

By default, `xx` refuses to replace an existing output directory. Choose a replacement policy when the destination exists:

```console
$ xx --rm source.tar.gz
$ xx --overwrite source.tar.gz
```

`--rm` removes the old directory only after extraction and validation succeed. `-O/--overwrite` merges the extracted tree into the old directory, replaces conflicts, and retains unrelated files. These options are mutually exclusive.

### Logical roots

`xx` removes one redundant directory from the top of the extracted tree. If the staging root contains exactly one directory, `xx` atomically moves that directory as the output root. Otherwise, it moves the staging root. The name of the output directory is either derived from the filename of the archive, or the `-o/--output` option, if provided.

Use `-n/--no-container-dir` to extract directly into the current directory, or into the directory specified by `--output`, instead of creating an archive-named output directory:

```console
$ xx -n source.tar.gz
$ xx --no-container-dir --output vendor source.tar.gz
```

This mode merges extracted entries into the destination, replaces conflicts, and retains unrelated entries; `--overwrite` is not required. `--rm` cannot be combined with `--no-container-dir`.

For example, if `project.tar` contains only `project/src` and `project/Cargo.toml`, this command:

```console
$ xx --output work project.tar
```

creates `work/src` and `work/Cargo.toml`, not `work/project/src`.

### Listing

Listing archive contents without extracting them can be done using the `-l/--list` option:

```console
$ xx --list source.zip
```

Add metadata in an `tar tv`-style form:

```console
$ xx --list --verbose source.zip
$ xx -lv source.zip
```

Emit a JSON array for programmatic use using the `-J/--json` option:

```console
$ xx --list --json source.zip
$ xx --list --verbose --json source.zip
$ xx -lvJ source.zip
```

Use NUL separators when passing names to another program with the `-0` option:

```console
$ xx --list -0 source.zip
```

`-0` cannot be combined with `--json`. Listing writes data to standard output. Progress and errors go to standard error.

### Parallel extraction

Zip, jar, and 7z extraction processes independent entries in parallel if the `-t/--threads` option is provided and specifies more than one worker.

```console
$ xx --threads 8 source.zip
```

Stream-oriented formats, such as tar, use one extraction worker even when `--threads` is greater than one.

### Options

| Option | Purpose |
| --- | --- |
| `-F, --format <FORMAT>` | Select the archive format by name or suffix. |
| `--rm` | Replace an existing output directory after successful validation. |
| `-O, --overwrite` | Merge into an existing output directory. |
| `-t, --threads <N>` | Set the worker limit. The default is `1`. |
| `-l, --list` | List entries instead of extracting them. |
| `-v, --verbose` | Include metadata when listing, or report extracted files and the final publication when extracting. |
| `-0` | Terminate listing fields with NUL bytes. |
| `-J, --json` | Write the listing as one JSON array. |
| `-p, --progress` | Report extraction or listing phase, format, completed entries, and bytes on standard error. |
| `-o, --output <DIR>` | Set the output directory. |
| `-n, --no-container-dir` | Merge directly into the current or specified output directory, replacing conflicts. |
| `-h, --help` | Print help. |
| `-V, --version` | Print the version. |

## Supported formats

Suffix matching ignores ASCII case. The longest suffix wins.

| Format | Recognized suffixes |
| --- | --- |
| tar | `.tar`, `.tar.Z`, `.tar.gz`, `.tgz`, `.tar.bz2`, `.tbz2`, `.tar.zst`, `.tar.xz` |
| PAX | `.pax`, `.pax.Z`, `.pax.gz`, `.pax.bz2`, `.pax.zst`, `.pax.xz` |
| CPIO newc | `.cpio`, `.cpio.Z`, `.cpio.gz`, `.cpio.bz2`, `.cpio.zst`, `.cpio.xz` |
| ar | `.ar`, `.a` |
| 7z | `.7z` |
| ZIP | `.zip` |
| JAR | `.jar` |

`--format` also accepts aliases such as `tar`, `gzip`, `tar-gzip`, `tbz2`, `cpio-xz`, `7z`, `zip`, and `jar`.

Cpio support covers SVR4 `newc` and its CRC variant. The default 7z feature supports the codecs enabled in `Cargo.toml`. Zip support includes stored, Deflate, Bzip2, XZ, and Zstandard entries. Encrypted or unsupported entries fail with an error.

## Safety

`xx` treats archive contents as untrusted input:

1. It extracts into a private directory beside the destination.
2. It rejects absolute paths, `..` components, Windows drive paths, and UNC paths.
3. It uses platform-independent archive paths: both `/` and `\` are treated as separators on every host, so accepted archives produce the same logical directory structure everywhere.
4. It rejects links that resolve outside the staged tree.
5. It creates links only after ordinary files and directories.
6. It rejects device nodes, FIFOs, sockets, and other special files.
7. It (atomically) moves the staged tree only after extraction and validation succeed.

A backslash is therefore not preserved as part of a filename; it is interpreted as a separator even on Unix-like hosts.

`xx` never modifies the input archive. A create-only or replace failure leaves the existing output unchanged if it occurs before publication. An `--overwrite` failure may leave a partial merge because that mode changes the destination in place.

`xx` does not impose an extraction-size limit. Apply an external disk or resource limit when processing hostile archives that may contain decompression bombs.

## Exit status

| Status | Meaning |
| ---: | --- |
| `0` | Success. |
| `2` | Invalid command-line use, including an unknown explicit format. |
| `3` | Input could not be opened or no format could be inferred from the filename. |
| `4` | Corrupt or unsupported archive data. |
| `5` | Unsafe archive content. |
| `6` | Output or filesystem error. |

## Development

The `Justfile` provides the usual commands:

```console
$ just build
$ just test
$ just lint
$ just verify
```

## License

The package uses the MIT License.

## Notes

This project was created with AI assistance using various models.
