use crate::compress::UnixCompressReader;
use crate::error::{Result, XxError, archive_error};
use crate::model::{EntryKind, FormatId};
use bzip2::read::BzDecoder;

use flate2::read::GzDecoder;
use std::ffi::OsStr;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use tar::EntryType;
use xz2::read::XzDecoder;
use zip::read::ZipFile;

const SUFFIXES: &[(&str, FormatId)] = &[
    #[cfg(feature = "seven-z")]
    (".7z", FormatId::SevenZ),
    (".tar.gz", FormatId::TarGzip),
    (".tar.bz2", FormatId::TarBzip2),
    (".tbz2", FormatId::TarBzip2),
    (".tar.zst", FormatId::TarZstd),
    (".tar.xz", FormatId::TarXz),
    (".tar.z", FormatId::TarCompress),
    (".tgz", FormatId::TarGzip),
    (".tar", FormatId::Tar),
    (".pax.gz", FormatId::PaxGzip),
    (".pax.bz2", FormatId::PaxBzip2),
    (".pax.zst", FormatId::PaxZstd),
    (".pax.xz", FormatId::PaxXz),
    (".pax.z", FormatId::PaxCompress),
    (".pax", FormatId::Pax),
    (".cpio.gz", FormatId::CpioGzip),
    (".cpio.bz2", FormatId::CpioBzip2),
    (".cpio.zst", FormatId::CpioZstd),
    (".cpio.xz", FormatId::CpioXz),
    (".cpio.z", FormatId::CpioCompress),
    (".cpio", FormatId::Cpio),
    (".ar", FormatId::Ar),
    (".a", FormatId::Ar),
    (".zip", FormatId::Zip),
    (".jar", FormatId::Jar),
];

pub(crate) fn resolve_format(input: &Path, explicit: Option<&str>) -> Result<FormatId> {
    if let Some(value) = explicit {
        let normalized = value.trim().to_ascii_lowercase();
        let format = match normalized.as_str() {
            "tar" | ".tar" => FormatId::Tar,
            "tar-compress" | "compress" | "lzw" | "tar.z" | ".tar.z" => FormatId::TarCompress,
            "tar-gzip" | "gzip" | "tar.gz" | ".tar.gz" | "tgz" | ".tgz" => FormatId::TarGzip,
            "tar-bzip2" | "bzip2" | "tar.bz2" | ".tar.bz2" | "tbz2" | ".tbz2" => FormatId::TarBzip2,
            "tar-zstd" | "zstd" | "tar.zst" | ".tar.zst" => FormatId::TarZstd,
            "tar-xz" | "xz" | "tar.xz" | ".tar.xz" => FormatId::TarXz,
            "pax" | ".pax" => FormatId::Pax,
            "pax-compress" | "pax.z" | ".pax.z" => FormatId::PaxCompress,
            "pax-gzip" | "pax.gz" | ".pax.gz" => FormatId::PaxGzip,
            "pax-bzip2" | "pax.bz2" | ".pax.bz2" => FormatId::PaxBzip2,
            "pax-zstd" | "pax.zst" | ".pax.zst" => FormatId::PaxZstd,
            "pax-xz" | "pax.xz" | ".pax.xz" => FormatId::PaxXz,
            "cpio" | ".cpio" => FormatId::Cpio,
            "cpio-compress" | "cpio.z" | ".cpio.z" => FormatId::CpioCompress,
            "cpio-gzip" | "cpio.gz" | ".cpio.gz" => FormatId::CpioGzip,
            "cpio-bzip2" | "cpio.bz2" | ".cpio.bz2" => FormatId::CpioBzip2,
            "cpio-zstd" | "cpio.zst" | ".cpio.zst" => FormatId::CpioZstd,
            "cpio-xz" | "cpio.xz" | ".cpio.xz" => FormatId::CpioXz,
            "ar" | ".ar" | "a" | ".a" => FormatId::Ar,
            #[cfg(feature = "seven-z")]
            "7z" | ".7z" | "seven-z" | "sevenz" => FormatId::SevenZ,
            "zip" | ".zip" => FormatId::Zip,
            "jar" | ".jar" => FormatId::Jar,
            _ => return Err(XxError::UnsupportedFormat(value.into())),
        };
        return Ok(format);
    }

    let name = input
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    for &(suffix, format) in SUFFIXES {
        if name.ends_with(suffix) {
            return Ok(format);
        }
    }
    Err(XxError::FormatNotRecognized(input.to_path_buf()))
}

pub(crate) fn derive_output(input: &Path) -> Option<PathBuf> {
    let file_name = input.file_name()?.to_str()?.to_string();
    let lower = file_name.to_ascii_lowercase();
    let suffix = SUFFIXES
        .iter()
        .find(|(suffix, _)| lower.ends_with(*suffix))
        .map(|(suffix, _)| *suffix)?;
    let stem_len = file_name.len() - suffix.len();
    let stem = &file_name[..stem_len];
    Some(PathBuf::from(stem))
}
pub(crate) fn tar_kind(entry_type: EntryType) -> EntryKind {
    match entry_type {
        EntryType::Regular | EntryType::Continuous => EntryKind::File,
        EntryType::Directory => EntryKind::Directory,
        EntryType::Symlink => EntryKind::Symlink,
        EntryType::Link => EntryKind::Hardlink,
        _ => EntryKind::Special,
    }
}

pub(crate) fn cpio_kind(mode: u32) -> EntryKind {
    match mode & 0o170000 {
        0o040000 => EntryKind::Directory,
        0o100000 => EntryKind::File,
        0o120000 => EntryKind::Symlink,
        _ => EntryKind::Special,
    }
}

pub(crate) fn zip_kind<R: Read>(file: &ZipFile<'_, R>) -> EntryKind {
    if file.is_dir() {
        EntryKind::Directory
    } else if file.is_symlink() {
        EntryKind::Symlink
    } else if let Some(mode) = file.unix_mode() {
        let file_type = mode & 0o170000;
        if file_type != 0 && file_type != 0o100000 {
            EntryKind::Special
        } else {
            EntryKind::File
        }
    } else {
        EntryKind::File
    }
}

pub(crate) fn format_time(seconds: u64) -> Option<String> {
    let seconds = i64::try_from(seconds).ok()?;
    let days = seconds.div_euclid(86_400);
    let day_seconds = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days)?;
    let hour = day_seconds / 3_600;
    let minute = (day_seconds % 3_600) / 60;
    let second = day_seconds % 60;
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z"
    ))
}

pub(crate) fn format_zip_time(value: zip::DateTime) -> Option<String> {
    if value.month() == 0 || value.day() == 0 || value.month() > 12 || value.day() > 31 {
        return None;
    }
    Some(format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        value.year(),
        value.month(),
        value.day(),
        value.hour(),
        value.minute(),
        value.second()
    ))
}

pub(crate) fn zip_time_seconds(value: zip::DateTime) -> Option<u64> {
    let days = days_from_civil(
        i64::from(value.year()),
        u32::from(value.month()),
        u32::from(value.day()),
    )?;
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(i64::from(value.hour()) * 3_600)?
        .checked_add(i64::from(value.minute()) * 60)?
        .checked_add(i64::from(value.second()))?;
    u64::try_from(seconds).ok()
}

#[cfg(feature = "seven-z")]
pub(crate) fn seven_z_seconds(value: sevenz_rust2::NtTime) -> Option<u64> {
    let system_time: std::time::SystemTime = value.into();
    system_time
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_secs())
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || day == 0 || day > 31 {
        return None;
    }
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 }.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
}

fn civil_from_days(days: i64) -> Option<(i64, i64, i64)> {
    let z = days.checked_add(719_468)?;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    Some((year, month, day))
}

pub(crate) fn open_archive_reader(format: FormatId, input: &Path) -> Result<Box<dyn Read>> {
    let file = File::open(input).map_err(|source| XxError::InputIo {
        path: input.to_path_buf(),
        source,
    })?;
    match format {
        FormatId::Tar | FormatId::Pax | FormatId::Cpio | FormatId::Ar => Ok(Box::new(file)),
        FormatId::TarGzip | FormatId::PaxGzip | FormatId::CpioGzip => {
            Ok(Box::new(GzDecoder::new(file)))
        }
        FormatId::TarBzip2 | FormatId::PaxBzip2 | FormatId::CpioBzip2 => {
            Ok(Box::new(BzDecoder::new(file)))
        }
        FormatId::TarZstd | FormatId::PaxZstd | FormatId::CpioZstd => {
            zstd::stream::read::Decoder::new(file)
                .map(|decoder| Box::new(decoder) as Box<dyn Read>)
                .map_err(|error| archive_error(None, error))
        }
        FormatId::TarXz | FormatId::PaxXz | FormatId::CpioXz => Ok(Box::new(XzDecoder::new(file))),
        FormatId::TarCompress | FormatId::PaxCompress | FormatId::CpioCompress => {
            Ok(Box::new(UnixCompressReader::new(file)))
        }
        #[cfg(feature = "seven-z")]
        FormatId::SevenZ => Err(XxError::UnsupportedFormat(format.label().into())),
        FormatId::Zip | FormatId::Jar => Err(XxError::UnsupportedFormat(format.label().into())),
    }
}
