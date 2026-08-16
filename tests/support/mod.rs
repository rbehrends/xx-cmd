use std::ffi::OsStr;
use std::fs;
use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub const ROOT: &str = "bundle";
pub const FILE_PATH: &str = "bundle/nested/file.txt";
pub const FILE_CONTENT: &[u8] = b"xx integration fixture\n";
pub const FILE_MTIME: u64 = 1_700_000_000;
pub const DIRECTORY_MTIME: u64 = 1_700_000_001;
pub const EXECUTABLE_PATH: &str = "bundle/bin/run.sh";
pub const EXECUTABLE_CONTENT: &[u8] = b"#!/bin/sh\necho fixture\n";
pub const SYMLINK_PATH: &str = "bundle/current";
pub const SYMLINK_TARGET: &str = "nested/file.txt";

pub fn reset_payload() -> Vec<u8> {
    let mut state = 0x1234_5678_u32;
    let mut output = Vec::with_capacity(131_072);
    for _ in 0..131_072 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        output.push((state >> 24) as u8);
    }
    output
}

pub fn run_xx<I, S>(working_directory: &Path, args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_xx"))
        .current_dir(working_directory)
        .args(args)
        .output()
        .expect("failed to launch xx")
}

pub fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "xx failed with {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn assert_exit(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "unexpected status; stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn write_standard_tar(path: &Path) -> io::Result<()> {
    write_tar(path, true)
}

pub fn standard_tar_bytes(include_root: bool) -> io::Result<Vec<u8>> {
    let mut builder = tar::Builder::new(Vec::new());
    if include_root {
        append_directory(&mut builder, ROOT, 0o755, DIRECTORY_MTIME)?;
    }
    append_directory(&mut builder, "bundle/nested", 0o755, DIRECTORY_MTIME)?;
    append_file(&mut builder, FILE_PATH, FILE_CONTENT, 0o644, FILE_MTIME)?;
    append_directory(&mut builder, "bundle/bin", 0o755, DIRECTORY_MTIME)?;
    append_file(
        &mut builder,
        EXECUTABLE_PATH,
        EXECUTABLE_CONTENT,
        0o755,
        FILE_MTIME,
    )?;
    #[cfg(unix)]
    append_symlink(&mut builder, SYMLINK_PATH, SYMLINK_TARGET, FILE_MTIME)?;
    builder.into_inner()
}

pub fn write_tar(path: &Path, include_root: bool) -> io::Result<()> {
    fs::write(path, standard_tar_bytes(include_root)?)
}

pub fn write_tar_gzip(path: &Path) -> io::Result<()> {
    let data = standard_tar_bytes(true)?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&data)?;
    fs::write(path, encoder.finish()?)
}

pub fn write_tar_bzip2(path: &Path) -> io::Result<()> {
    let data = standard_tar_bytes(true)?;
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    encoder.write_all(&data)?;
    fs::write(path, encoder.finish()?)
}

pub fn write_tar_xz(path: &Path) -> io::Result<()> {
    let data = standard_tar_bytes(true)?;
    let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 6);
    encoder.write_all(&data)?;
    fs::write(path, encoder.finish()?)
}

pub fn write_tar_zstd(path: &Path) -> io::Result<()> {
    let data = standard_tar_bytes(true)?;
    fs::write(path, zstd::stream::encode_all(Cursor::new(data), 3)?)
}

pub fn write_cpio(path: &Path) -> io::Result<()> {
    let mut entries = vec![
        (
            cpio::NewcBuilder::new("bundle")
                .mode(0o040755)
                .mtime(DIRECTORY_MTIME as u32),
            Cursor::new(Vec::new()),
        ),
        (
            cpio::NewcBuilder::new(FILE_PATH)
                .mode(0o100644)
                .mtime(FILE_MTIME as u32),
            Cursor::new(FILE_CONTENT.to_vec()),
        ),
        (
            cpio::NewcBuilder::new(EXECUTABLE_PATH)
                .mode(0o100755)
                .mtime(FILE_MTIME as u32),
            Cursor::new(EXECUTABLE_CONTENT.to_vec()),
        ),
    ];
    #[cfg(unix)]
    entries.push((
        cpio::NewcBuilder::new(SYMLINK_PATH)
            .mode(0o120777)
            .mtime(FILE_MTIME as u32),
        Cursor::new(SYMLINK_TARGET.as_bytes().to_vec()),
    ));
    let bytes = cpio::write_cpio(entries.into_iter(), Cursor::new(Vec::new()))?.into_inner();
    fs::write(path, bytes)
}

pub fn write_ar(path: &Path) -> io::Result<()> {
    let bytes = {
        let mut bytes = Vec::new();
        {
            let mut builder = ar::Builder::new(&mut bytes);
            let mut header =
                ar::Header::new(b"bundle/file.txt".to_vec(), FILE_CONTENT.len() as u64);
            header.set_mode(0o100644);
            header.set_mtime(FILE_MTIME);
            builder
                .append(&header, Cursor::new(FILE_CONTENT))
                .map_err(io::Error::other)?;
        }
        bytes
    };
    fs::write(path, bytes)
}

pub fn write_zip(path: &Path, jar: bool) -> io::Result<()> {
    use zip::CompressionMethod;
    use zip::write::{SimpleFileOptions, ZipWriter};

    let output = std::fs::File::create(path)?;
    let mut writer = ZipWriter::new(output);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let directory_options = options.unix_permissions(0o755);
    writer
        .add_directory("bundle/", directory_options)
        .map_err(io::Error::other)?;
    writer
        .add_directory("bundle/nested/", directory_options)
        .map_err(io::Error::other)?;
    writer
        .start_file("bundle/nested/file.txt", options)
        .map_err(io::Error::other)?;
    writer.write_all(FILE_CONTENT)?;
    if !jar {
        writer
            .add_directory("bundle/bin/", directory_options)
            .map_err(io::Error::other)?;
        writer
            .start_file("bundle/bin/run.sh", options.unix_permissions(0o755))
            .map_err(io::Error::other)?;
        writer.write_all(EXECUTABLE_CONTENT)?;
    }
    writer.finish().map(|_| ()).map_err(io::Error::other)
}

pub fn write_zip_with_entries(path: &Path, entries: &[(&str, &[u8])]) -> io::Result<()> {
    use zip::CompressionMethod;
    use zip::write::{SimpleFileOptions, ZipWriter};

    let output = std::fs::File::create(path)?;
    let mut writer = ZipWriter::new(output);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    for (name, data) in entries {
        writer
            .start_file(*name, options)
            .map_err(io::Error::other)?;
        writer.write_all(data)?;
    }
    writer.finish().map(|_| ()).map_err(io::Error::other)
}

fn append_directory<W: Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    mode: u32,
    mtime: u64,
) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_path(path)?;
    header.set_entry_type(tar::EntryType::Directory);
    header.set_mode(mode);
    header.set_mtime(mtime);
    header.set_size(0);
    header.set_cksum();
    builder.append(&header, io::empty())
}

fn append_file<W: Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    data: &[u8],
    mode: u32,
    mtime: u64,
) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_path(path)?;
    header.set_entry_type(tar::EntryType::Regular);
    header.set_mode(mode);
    header.set_mtime(mtime);
    header.set_size(data.len() as u64);
    header.set_cksum();
    builder.append(&header, data)
}

fn append_symlink<W: Write>(
    builder: &mut tar::Builder<W>,
    path: &str,
    target: &str,
    mtime: u64,
) -> io::Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_path(path)?;
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_link_name(target)?;
    header.set_mode(0o777);
    header.set_mtime(mtime);
    header.set_size(0);
    header.set_cksum();
    builder.append(&header, io::empty())
}

pub fn file_mtime(path: &Path) -> i64 {
    filetime::FileTime::from_last_modification_time(&fs::metadata(path).unwrap()).unix_seconds()
}

pub fn read_tree(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = Vec::new();
    collect_tree(root, root, &mut entries);
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    entries
}

fn collect_tree(root: &Path, current: &Path, entries: &mut Vec<(PathBuf, Vec<u8>)>) {
    let Ok(read_dir) = fs::read_dir(current) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        let relative = path.strip_prefix(root).unwrap().to_path_buf();
        let metadata = fs::symlink_metadata(&path).unwrap();
        if metadata.is_dir() {
            entries.push((relative, Vec::new()));
            collect_tree(root, &path, entries);
        } else if metadata.file_type().is_symlink() {
            entries.push((
                relative,
                fs::read_link(path)
                    .unwrap()
                    .as_os_str()
                    .as_encoded_bytes()
                    .to_vec(),
            ));
        } else {
            entries.push((relative, fs::read(path).unwrap()));
        }
    }
}
