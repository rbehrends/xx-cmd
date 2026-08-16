use crate::error::{Result, XxError, archive_error, output_error};
#[cfg(feature = "seven-z")]
use crate::format::seven_z_seconds;
use crate::format::{
    cpio_kind, format_time, format_zip_time, open_archive_reader, tar_kind, zip_kind,
};
use crate::model::{
    EntryKind, EntryRecord, FormatId, JsonRecord, ListDelimiter, ListFormat, ListKind, Request,
};
use crate::safety::mode_string;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;
use tar::EntryType;
use zip::read::ZipArchive;

pub(crate) fn list_archive(request: &Request) -> Result<Vec<EntryRecord>> {
    if request.progress {
        eprintln!(
            "xx: phase=list format={} completed=0 entries bytes=0",
            request.format.label()
        );
    }
    let records = match request.format {
        FormatId::Zip | FormatId::Jar => list_zip(&request.input),
        FormatId::Ar => list_ar(&request.input),
        #[cfg(feature = "seven-z")]
        FormatId::SevenZ => list_seven_z(&request.input),
        FormatId::Cpio
        | FormatId::CpioCompress
        | FormatId::CpioGzip
        | FormatId::CpioBzip2
        | FormatId::CpioZstd
        | FormatId::CpioXz => {
            let reader = open_archive_reader(request.format, &request.input)?;
            list_cpio(reader)
        }
        _ => {
            let reader = open_archive_reader(request.format, &request.input)?;
            list_tar(reader)
        }
    }?;
    if request.progress {
        let bytes = records
            .iter()
            .filter_map(|record| record.size)
            .fold(0_u64, u64::saturating_add);
        eprintln!(
            "xx: phase=list format={} completed={} entries bytes={bytes}",
            request.format.label(),
            records.len()
        );
    }
    Ok(records)
}

fn list_tar<R: Read>(reader: R) -> Result<Vec<EntryRecord>> {
    let mut archive = tar::Archive::new(reader);
    let mut records = Vec::new();
    let entries = archive
        .entries()
        .map_err(|error| archive_error(None, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| archive_error(None, error))?;
        let header = entry.header();
        let entry_type = header.entry_type();
        if matches!(
            entry_type,
            EntryType::XHeader
                | EntryType::XGlobalHeader
                | EntryType::GNULongName
                | EntryType::GNULongLink
        ) {
            continue;
        }
        let name = entry.path_bytes().to_vec();
        let kind = tar_kind(entry_type);
        let target = if matches!(kind, EntryKind::Symlink | EntryKind::Hardlink) {
            entry.link_name_bytes().map(|value| value.to_vec())
        } else {
            None
        };
        records.push(EntryRecord {
            name,
            kind,
            size: if kind == EntryKind::File {
                Some(header.size().map_err(|error| archive_error(None, error))?)
            } else {
                None
            },
            perm: header.mode().ok().map(mode_string),
            mtime: header.mtime().ok().and_then(format_time),
            link_target: target,
        });
    }
    Ok(records)
}

fn list_cpio<R: Read>(reader: R) -> Result<Vec<EntryRecord>> {
    let mut reader = cpio::NewcReader::new(reader).map_err(|error| archive_error(None, error))?;
    let mut records = Vec::new();
    loop {
        let entry = reader.entry().clone();
        if entry.is_trailer() {
            break;
        }
        let name = entry.name().as_bytes().to_vec();
        let kind = cpio_kind(entry.mode());
        let target = if kind == EntryKind::Symlink {
            let mut target = Vec::new();
            reader
                .read_to_end(&mut target)
                .map_err(|error| archive_error(Some(&name), error))?;
            Some(target)
        } else {
            None
        };
        records.push(EntryRecord {
            name,
            kind,
            size: if kind == EntryKind::File {
                Some(u64::from(entry.file_size()))
            } else {
                None
            },
            perm: Some(mode_string(entry.mode())),
            mtime: format_time(u64::from(entry.mtime())),
            link_target: target,
        });
        let inner = reader
            .finish()
            .map_err(|error| archive_error(None, error))?;
        reader = cpio::NewcReader::new(inner).map_err(|error| archive_error(None, error))?;
    }
    Ok(records)
}

fn list_ar(input: &Path) -> Result<Vec<EntryRecord>> {
    let file = File::open(input).map_err(|source| XxError::InputIo {
        path: input.to_path_buf(),
        source,
    })?;
    let mut archive = ar::Archive::new(file);
    let mut records = Vec::new();
    while let Some(entry) = archive.next_entry() {
        let mut entry = entry.map_err(|error| archive_error(None, error))?;
        let header = entry.header();
        let name = header.identifier().to_vec();
        records.push(EntryRecord {
            name: name.clone(),
            kind: EntryKind::File,
            size: Some(header.size()),
            perm: Some(mode_string(header.mode())),
            mtime: format_time(header.mtime()),
            link_target: None,
        });
        io::copy(&mut entry, &mut io::sink()).map_err(|error| archive_error(Some(&name), error))?;
    }
    Ok(records)
}

#[cfg(feature = "seven-z")]
fn list_seven_z(input: &Path) -> Result<Vec<EntryRecord>> {
    let archive = sevenz_rust2::ArchiveReader::open(input, sevenz_rust2::Password::empty())
        .map_err(|error| archive_error(None, error))?;
    let records = archive
        .archive()
        .files
        .iter()
        .map(|entry| {
            let kind = if entry.is_anti_item {
                EntryKind::Special
            } else if entry.is_directory {
                EntryKind::Directory
            } else {
                EntryKind::File
            };
            EntryRecord {
                name: entry.name.as_bytes().to_vec(),
                kind,
                size: if kind == EntryKind::File {
                    Some(entry.size)
                } else {
                    None
                },
                perm: None,
                mtime: if entry.has_last_modified_date {
                    seven_z_seconds(entry.last_modified_date).and_then(format_time)
                } else {
                    None
                },
                link_target: None,
            }
        })
        .collect();
    Ok(records)
}

fn list_zip(input: &Path) -> Result<Vec<EntryRecord>> {
    let file = File::open(input).map_err(|source| XxError::InputIo {
        path: input.to_path_buf(),
        source,
    })?;
    let mut archive = ZipArchive::new(file).map_err(|error| archive_error(None, error))?;
    let mut records = Vec::with_capacity(archive.len());
    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| archive_error(None, error))?;
        let name = file.name_raw().to_vec();
        let kind = zip_kind(&file);
        let target = if matches!(kind, EntryKind::Symlink | EntryKind::Hardlink) {
            let mut target = Vec::new();
            file.read_to_end(&mut target)
                .map_err(|error| archive_error(None, error))?;
            Some(target)
        } else {
            None
        };
        let mtime = file.last_modified().and_then(format_zip_time);
        records.push(EntryRecord {
            name,
            kind,
            size: if kind == EntryKind::File {
                Some(file.size())
            } else {
                None
            },
            perm: file.unix_mode().map(mode_string),
            mtime,
            link_target: target,
        });
    }
    Ok(records)
}

pub(crate) fn render_listing(
    records: &[EntryRecord],
    kind: ListKind,
    format: ListFormat,
    delimiter: ListDelimiter,
) -> Result<()> {
    let stdout = io::stdout();
    let mut output = stdout.lock();
    match format {
        ListFormat::Text => {
            for record in records {
                if kind == ListKind::Names {
                    output.write_all(&record.name).map_err(output_error)?;
                } else if delimiter == ListDelimiter::Line {
                    output
                        .write_all(human_mode(record.kind, record.perm.as_deref()).as_bytes())
                        .map_err(output_error)?;
                    output.write_all(b" ").map_err(output_error)?;
                    output
                        .write_all(human_size(record.size).as_bytes())
                        .map_err(output_error)?;
                    output.write_all(b"  ").map_err(output_error)?;
                    let human_mtime =
                        human_time(record.mtime.as_deref()).unwrap_or_else(|| "-".to_owned());
                    output
                        .write_all(human_mtime.as_bytes())
                        .map_err(output_error)?;
                    output.write_all(b"  ").map_err(output_error)?;
                    output.write_all(&record.name).map_err(output_error)?;
                    if let Some(target) = &record.link_target {
                        output.write_all(b" -> ").map_err(output_error)?;
                        output.write_all(target).map_err(output_error)?;
                    }
                } else {
                    output.write_all(&record.name).map_err(output_error)?;
                    output.write_all(b"\0").map_err(output_error)?;
                    output
                        .write_all(record.kind.label().as_bytes())
                        .map_err(output_error)?;
                    output.write_all(b"\0").map_err(output_error)?;
                    write_optional_bytes(
                        &mut output,
                        record.size.map(|value| value.to_string()).as_deref(),
                        delimiter,
                    )?;
                    write_optional_bytes(&mut output, record.perm.as_deref(), delimiter)?;
                    write_optional_bytes(&mut output, record.mtime.as_deref(), delimiter)?;
                    write_optional_bytes(
                        &mut output,
                        record
                            .link_target
                            .as_deref()
                            .map(bytes_to_lossless_string)
                            .transpose()?,
                        delimiter,
                    )?;
                }
                if kind == ListKind::Names || delimiter == ListDelimiter::Line {
                    output
                        .write_all(if delimiter == ListDelimiter::Nul {
                            b"\0"
                        } else {
                            b"\n"
                        })
                        .map_err(output_error)?;
                }
            }
        }
        ListFormat::Json => {
            let json_records = records.iter().map(to_json_record).collect::<Vec<_>>();
            render_json(&mut output, &json_records, kind)?;
        }
    }
    output.flush().map_err(output_error)?;
    Ok(())
}

pub(crate) fn render_json<W: Write>(
    output: &mut W,
    records: &[JsonRecord],
    kind: ListKind,
) -> Result<()> {
    if records.is_empty() {
        output.write_all(b"[]\n").map_err(output_error)?;
        return Ok(());
    }

    output.write_all(b"[\n").map_err(output_error)?;
    for (index, record) in records.iter().enumerate() {
        output.write_all(b"  ").map_err(output_error)?;
        if kind == ListKind::Names {
            write_json_bytes(output, &record.name)?;
        } else {
            write_json_record(output, record)?;
        }
        if index + 1 < records.len() {
            output.write_all(b",").map_err(output_error)?;
        }
        output.write_all(b"\n").map_err(output_error)?;
    }
    output.write_all(b"]\n").map_err(output_error)?;
    Ok(())
}

fn write_json_record<W: Write>(output: &mut W, record: &JsonRecord) -> Result<()> {
    output.write_all(b"{").map_err(output_error)?;
    write_json_bytes_field(output, "name", Some(&record.name), true)?;
    write_json_field(output, "type", &record.kind, false)?;
    write_json_field(output, "size", &record.size, false)?;
    write_json_field(output, "perm", &record.perm, false)?;
    write_json_field(output, "mtime", &record.mtime, false)?;
    write_json_bytes_field(output, "link_target", record.link_target.as_deref(), false)?;
    output.write_all(b"}").map_err(output_error)?;
    Ok(())
}

fn write_json_field<W: Write, T: serde::Serialize>(
    output: &mut W,
    name: &str,
    value: &T,
    first: bool,
) -> Result<()> {
    if !first {
        output.write_all(b", ").map_err(output_error)?;
    }
    write_json_value(output, name)?;
    output.write_all(b": ").map_err(output_error)?;
    write_json_value(output, value)
}

fn write_json_value<W: Write, T: serde::Serialize + ?Sized>(
    output: &mut W,
    value: &T,
) -> Result<()> {
    serde_json::to_writer(output, value).map_err(|error| output_error(io::Error::other(error)))
}

fn write_json_bytes_field<W: Write>(
    output: &mut W,
    name: &str,
    value: Option<&[u8]>,
    first: bool,
) -> Result<()> {
    if !first {
        output.write_all(b", ").map_err(output_error)?;
    }
    write_json_value(output, name)?;
    output.write_all(b": ").map_err(output_error)?;
    match value {
        Some(value) => write_json_bytes(output, value),
        None => output.write_all(b"null").map_err(output_error),
    }
}

fn write_json_bytes<W: Write>(output: &mut W, bytes: &[u8]) -> Result<()> {
    output.write_all(b"\"").map_err(output_error)?;
    let mut offset = 0;
    while offset < bytes.len() {
        match std::str::from_utf8(&bytes[offset..]) {
            Ok(value) => {
                write_json_text(output, value)?;
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid > 0 {
                    let value = std::str::from_utf8(&bytes[offset..offset + valid])
                        .expect("valid UTF-8 prefix");
                    write_json_text(output, value)?;
                    offset += valid;
                }
                if offset < bytes.len() {
                    write!(output, "\\udc{:02x}", bytes[offset]).map_err(output_error)?;
                    offset += 1;
                }
            }
        }
    }
    output.write_all(b"\"").map_err(output_error)
}

fn write_json_text<W: Write>(output: &mut W, value: &str) -> Result<()> {
    for character in value.chars() {
        match character {
            '"' => output.write_all(b"\\\"").map_err(output_error)?,
            '\\' => output.write_all(b"\\\\").map_err(output_error)?,
            '\u{08}' => output.write_all(b"\\b").map_err(output_error)?,
            '\u{0c}' => output.write_all(b"\\f").map_err(output_error)?,
            '\n' => output.write_all(b"\\n").map_err(output_error)?,
            '\r' => output.write_all(b"\\r").map_err(output_error)?,
            '\t' => output.write_all(b"\\t").map_err(output_error)?,
            character if character <= '\u{1f}' => {
                write!(output, "\\u{:04x}", character as u32).map_err(output_error)?;
            }
            character => {
                let mut encoded = [0; 4];
                output
                    .write_all(character.encode_utf8(&mut encoded).as_bytes())
                    .map_err(output_error)?;
            }
        }
    }
    Ok(())
}

fn write_optional_bytes<W: Write>(
    output: &mut W,
    value: Option<&str>,
    delimiter: ListDelimiter,
) -> Result<()> {
    if let Some(value) = value {
        output.write_all(value.as_bytes()).map_err(output_error)?;
    } else {
        output.write_all(b"-").map_err(output_error)?;
    }
    output
        .write_all(if delimiter == ListDelimiter::Nul {
            b"\0"
        } else {
            b"\t"
        })
        .map_err(output_error)?;
    Ok(())
}

fn to_json_record(record: &EntryRecord) -> JsonRecord {
    JsonRecord {
        name: record.name.clone(),
        kind: record.kind.label().into(),
        size: record.size,
        perm: record.perm.clone(),
        mtime: record.mtime.clone(),
        link_target: record.link_target.clone(),
    }
}

fn bytes_to_lossless_string(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes).map_err(|_| XxError::UnrepresentableOutput {
        path: bytes.to_vec(),
        reason: "not valid UTF-8".into(),
    })
}

pub(crate) fn human_mode(kind: EntryKind, permissions: Option<&str>) -> String {
    let type_char = match kind {
        EntryKind::File => '-',
        EntryKind::Directory => 'd',
        EntryKind::Symlink => 'l',
        EntryKind::Hardlink => 'h',
        EntryKind::Special => '?',
    };
    let Some(mode) = permissions.and_then(|value| u32::from_str_radix(value, 8).ok()) else {
        return format!("{type_char}---------");
    };

    let mut result = String::with_capacity(10);
    result.push(type_char);
    result.push(if mode & 0o400 != 0 { 'r' } else { '-' });
    result.push(if mode & 0o200 != 0 { 'w' } else { '-' });
    result.push(permission_marker(mode, 0o100, 0o4000, 's', 'S'));
    result.push(if mode & 0o040 != 0 { 'r' } else { '-' });
    result.push(if mode & 0o020 != 0 { 'w' } else { '-' });
    result.push(permission_marker(mode, 0o010, 0o2000, 's', 'S'));
    result.push(if mode & 0o004 != 0 { 'r' } else { '-' });
    result.push(if mode & 0o002 != 0 { 'w' } else { '-' });
    result.push(permission_marker(mode, 0o001, 0o1000, 't', 'T'));
    result
}

fn permission_marker(
    mode: u32,
    execute_bit: u32,
    special_bit: u32,
    enabled: char,
    disabled: char,
) -> char {
    match (mode & execute_bit != 0, mode & special_bit != 0) {
        (true, true) => enabled,
        (false, true) => disabled,
        (true, false) => 'x',
        (false, false) => '-',
    }
}

pub(crate) fn human_size(value: Option<u64>) -> String {
    let value = value.map_or_else(|| "-".to_owned(), |value| value.to_string());
    format!("{value:>10}")
}

pub(crate) fn human_time(value: Option<&str>) -> Option<String> {
    let value = value?;
    let value = value.strip_suffix('Z')?;
    let (date, _) = value.split_once('T')?;
    let mut components = date.split('-');
    let year = components.next()?.parse::<u32>().ok()?;
    let month = components.next()?.parse::<usize>().ok()?;
    let day = components.next()?.parse::<u32>().ok()?;
    if components.next().is_some() || !(1..=31).contains(&day) {
        return None;
    }
    let month = match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => return None,
    };
    Some(format!("{month} {day:>2} {year:04}"))
}
