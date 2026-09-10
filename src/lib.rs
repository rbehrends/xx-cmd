mod app;
mod cli;
mod commit;
mod compress;
mod error;
mod extract;
mod format;
mod listing;
mod model;
mod path;
mod safety;

pub use app::cli_main;
pub use error::XxError;

#[cfg(test)]
mod tests {
    use super::XxError;
    use super::app::{output_parent, validate_output_parent};
    use super::error::exit_code;
    use super::extract::{display_extraction_path, human_extract_size, normalize_display_path};
    use super::format::{derive_output, resolve_format};
    use super::listing::{human_mode, human_size, human_time, render_json};
    use super::model::{EntryKind, FormatId, JsonRecord, ListKind, OutputMode};
    use super::path::{normalize_path, validate_link_target};
    use std::path::{Path, PathBuf};

    #[test]
    fn resolves_longest_suffix() {
        assert_eq!(
            resolve_format(Path::new("a.tar.gz"), None).unwrap(),
            FormatId::TarGzip
        );
        assert_eq!(
            derive_output(Path::new("a.tar.gz")).unwrap(),
            PathBuf::from("a")
        );
        assert_eq!(
            derive_output(Path::new("archives/a.tar.gz")).unwrap(),
            PathBuf::from("a")
        );
        assert_eq!(
            resolve_format(Path::new("a.tbz2"), None).unwrap(),
            FormatId::TarBzip2
        );
        assert_eq!(
            resolve_format(Path::new("archive.data"), Some(".tbz2")).unwrap(),
            FormatId::TarBzip2
        );
        assert_eq!(
            derive_output(Path::new("a.tbz2")).unwrap(),
            PathBuf::from("a")
        );
        assert_eq!(
            resolve_format(Path::new("a.pax.gz"), None).unwrap(),
            FormatId::PaxGzip
        );
        assert_eq!(
            resolve_format(Path::new("a.cpio.xz"), None).unwrap(),
            FormatId::CpioXz
        );
        assert_eq!(
            resolve_format(Path::new("lib.a"), None).unwrap(),
            FormatId::Ar
        );
        assert_eq!(
            derive_output(Path::new("lib.a")).unwrap(),
            PathBuf::from("lib")
        );
        #[cfg(feature = "seven-z")]
        {
            assert_eq!(
                resolve_format(Path::new("archive.7z"), None).unwrap(),
                FormatId::SevenZ
            );
            assert_eq!(
                derive_output(Path::new("archive.7z")).unwrap(),
                PathBuf::from("archive")
            );
        }
    }

    #[test]
    fn explicit_unknown_format_is_a_usage_error() {
        assert_eq!(exit_code(&XxError::UnsupportedFormat("wat".into())), 2);
        assert_eq!(
            exit_code(&XxError::FormatNotRecognized(PathBuf::from("archive"))),
            3
        );
    }

    #[test]
    fn rejects_replace_when_output_contains_input_archive() {
        let temp = tempfile::tempdir().unwrap();
        let output = temp.path().join("pkg");
        std::fs::create_dir(&output).unwrap();
        let input = output.join("archive.tar.gz");
        std::fs::write(&input, b"archive").unwrap();

        let error = validate_output_parent(&output, &input, OutputMode::Replace).unwrap_err();
        assert!(matches!(
            error,
            XxError::OutputConflict(path, reason)
                if path == output && reason == "output contains the input archive"
        ));
        assert!(input.exists());
    }

    #[test]
    fn uses_current_directory_for_bare_output_name() {
        assert_eq!(output_parent(Path::new("test")), Path::new("."));
        assert_eq!(
            output_parent(Path::new("archives/test")),
            Path::new("archives")
        );
    }

    #[test]
    fn rejects_traversal() {
        assert!(normalize_path(b"../etc/passwd").is_err());
        assert!(normalize_path(b"..\\etc\\passwd").is_err());
        assert!(normalize_path(b"/etc/passwd").is_err());
        assert!(normalize_path(b"\\etc\\passwd").is_err());
        assert!(normalize_path(b"C:foo").is_err());
        assert!(normalize_path(b"C:foo\\evil.bat").is_err());
        assert_eq!(
            normalize_path(b"safe\\nested/file").unwrap().components,
            vec![b"safe".to_vec(), b"nested".to_vec(), b"file".to_vec()]
        );
    }

    #[test]
    fn safe_link_can_resolve_inside_root() {
        let path = normalize_path(b"dir/link").unwrap();
        assert!(validate_link_target(&path, b"../file").is_ok());
        assert!(validate_link_target(&path, b"..\\file").is_ok());
        assert!(validate_link_target(&path, b"../../file").is_err());
        assert!(validate_link_target(&path, b"..\\..\\file").is_err());
    }

    #[test]
    fn formats_ls_style_modes() {
        assert_eq!(human_mode(EntryKind::Directory, Some("0755")), "drwxr-xr-x");
        assert_eq!(human_mode(EntryKind::File, Some("0644")), "-rw-r--r--");
        assert_eq!(human_mode(EntryKind::Symlink, Some("0777")), "lrwxrwxrwx");
        assert_eq!(human_mode(EntryKind::File, Some("4755")), "-rwsr-xr-x");
    }

    #[test]
    fn normalizes_extraction_move_paths() {
        assert_eq!(
            normalize_display_path(Path::new("/tmp/./.xx-test")),
            PathBuf::from("/tmp/.xx-test")
        );
    }

    #[test]
    fn formats_extraction_directory_paths_with_native_separators() {
        let path = normalize_path(b"dir/subdir").unwrap();
        let separator = std::path::MAIN_SEPARATOR;
        assert_eq!(
            display_extraction_path(&path, false),
            format!("dir{separator}subdir")
        );
        assert_eq!(
            display_extraction_path(&path, true),
            format!("dir{separator}subdir{separator}")
        );
    }

    #[test]
    fn formats_human_extraction_sizes() {
        assert_eq!(human_extract_size(123), "123");
        assert_eq!(human_extract_size(12 * 1024), "12k");
        assert_eq!(human_extract_size(1536), "1.5k");
        assert_eq!(human_extract_size(12 * 1024 * 1024), "12M");
    }

    #[test]
    fn formats_human_listing_fields() {
        assert_eq!(human_size(Some(123)), "       123");
        assert_eq!(human_size(Some(12_345_678_901)), "12345678901");
        assert_eq!(human_size(None), "         -");
        assert_eq!(
            human_time(Some("2015-04-12T00:00:00Z")),
            Some("Apr 12 2015".to_owned())
        );
        assert_eq!(
            human_time(Some("2015-04-04T00:00:00Z")),
            Some("Apr  4 2015".to_owned())
        );
    }

    #[test]
    fn renders_readable_json_names() {
        let records = vec![
            JsonRecord {
                name: b"first".to_vec(),
                kind: "file".into(),
                size: Some(1),
                perm: None,
                mtime: None,
                link_target: None,
            },
            JsonRecord {
                name: b"second".to_vec(),
                kind: "file".into(),
                size: Some(2),
                perm: None,
                mtime: None,
                link_target: None,
            },
        ];
        let mut output = Vec::new();
        render_json(&mut output, &records, ListKind::Names).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[\n  \"first\",\n  \"second\"\n]\n"
        );
    }

    #[test]
    fn renders_readable_json_verbose_records() {
        let records = vec![JsonRecord {
            name: b"file,name".to_vec(),
            kind: "file".into(),
            size: Some(123),
            perm: Some("0644".into()),
            mtime: Some("2026-01-02T03:04:05Z".into()),
            link_target: None,
        }];
        let mut output = Vec::new();
        render_json(&mut output, &records, ListKind::Verbose).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[\n  {\"name\": \"file,name\", \"type\": \"file\", \"size\": 123, \"perm\": \"0644\", \"mtime\": \"2026-01-02T03:04:05Z\", \"link_target\": null}\n]\n"
        );
    }

    #[test]
    fn escapes_invalid_bytes_in_json_names_and_link_targets() {
        let records = vec![JsonRecord {
            name: vec![b'c', 0xff, b'"', b'\n'],
            kind: "symlink".into(),
            size: None,
            perm: None,
            mtime: None,
            link_target: Some(vec![b't', 0x80]),
        }];
        let mut output = Vec::new();
        render_json(&mut output, &records, ListKind::Verbose).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[\n  {\"name\": \"c\\udcff\\\"\\n\", \"type\": \"symlink\", \"size\": null, \"perm\": null, \"mtime\": null, \"link_target\": \"t\\udc80\"}\n]\n"
        );

        let mut output = Vec::new();
        render_json(&mut output, &records, ListKind::Names).unwrap();
        assert_eq!(
            String::from_utf8(output).unwrap(),
            "[\n  \"c\\udcff\\\"\\n\"\n]\n"
        );
    }
}
