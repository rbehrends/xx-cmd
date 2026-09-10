mod support;

macro_rules! xx_args {
    ([$($arg:expr),* $(,)?]) => {
        vec![$(std::ffi::OsString::from($arg)),*]
    };
}

use std::fs;
use std::path::Path;

type ArchiveWriter = fn(&Path) -> std::io::Result<()>;

use support::{
    EXECUTABLE_CONTENT, FILE_CONTENT, FILE_MTIME, ROOT, SYMLINK_TARGET, assert_exit,
    assert_success, file_mtime, read_tree, reset_payload, run_xx, write_ar, write_cpio,
    write_standard_tar, write_tar_bzip2, write_tar_gzip, write_tar_xz, write_tar_zstd, write_zip,
    write_zip_with_entries,
};

#[test]
fn derives_implicit_output_in_the_current_directory() {
    let temp = tempfile::tempdir().unwrap();
    let archive_directory = temp.path().join("archives");
    fs::create_dir(&archive_directory).unwrap();
    let archive = archive_directory.join("project.tar.gz");
    write_tar_gzip(&archive).unwrap();

    let output = run_xx(temp.path(), [archive.as_os_str()]);
    assert_success(&output);

    let extracted = temp.path().join("project");
    assert_eq!(
        fs::read(extracted.join("nested/file.txt")).unwrap(),
        FILE_CONTENT
    );
    assert!(!archive_directory.join("project").exists());
}

#[test]
fn extracts_the_supported_rust_generated_formats() {
    let cases: Vec<(&str, ArchiveWriter)> = vec![
        ("archive.tar", write_standard_tar),
        ("archive.tar.gz", write_tar_gzip),
        ("archive.tar.bz2", write_tar_bzip2),
        ("archive.tar.xz", write_tar_xz),
        ("archive.tar.zst", write_tar_zstd),
        ("archive.cpio", write_cpio),
        ("archive.ar", write_ar),
    ];

    for (archive_name, create_archive) in cases {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join(archive_name);
        create_archive(&archive).unwrap();

        let output = run_xx(temp.path(), [archive.as_os_str()]);
        assert_success(&output);

        let extracted = temp.path().join("archive");
        if archive_name == "archive.ar" {
            assert_eq!(fs::read(extracted.join("file.txt")).unwrap(), FILE_CONTENT);
            continue;
        }
        assert_eq!(
            fs::read(extracted.join("nested/file.txt")).unwrap(),
            FILE_CONTENT
        );
        assert_eq!(
            fs::read(extracted.join("bin/run.sh")).unwrap(),
            EXECUTABLE_CONTENT
        );
        assert!(extracted.join("nested").is_dir());
        assert_eq!(
            file_mtime(&extracted.join("nested/file.txt")),
            FILE_MTIME as i64
        );

        #[cfg(unix)]
        {
            let link = extracted.join("current");
            assert_eq!(fs::read_link(link).unwrap(), Path::new(SYMLINK_TARGET));
        }
    }
}

#[test]
fn extracts_a_real_unix_compress_fixture_after_a_dictionary_reset() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("reset.tar.Z");
    fs::write(&archive, include_bytes!("fixtures/external-reset.tar.Z")).unwrap();

    let output = run_xx(temp.path(), [archive.as_os_str()]);
    assert_success(&output);
    assert_eq!(
        fs::read(temp.path().join("reset/nested/payload.bin")).unwrap(),
        reset_payload()
    );
}

#[cfg(feature = "seven-z")]
#[test]
fn extracts_an_externally_produced_seven_z_fixture() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("external.7z");
    fs::write(&archive, include_bytes!("fixtures/external.7z")).unwrap();

    let output = run_xx(temp.path(), [archive.as_os_str()]);
    assert_success(&output);
    assert_eq!(
        fs::read(temp.path().join("external/nested/file.txt")).unwrap(),
        b"xx external fixture\n"
    );
}

#[test]
fn extracts_zip_and_jar_and_supports_explicit_format_for_unknown_suffix() {
    for (archive_name, jar) in [("archive.zip", false), ("archive.jar", true)] {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join(archive_name);
        write_zip(&archive, jar).unwrap();

        let output = run_xx(
            temp.path(),
            xx_args!([archive.as_os_str(), "--threads", "4"]),
        );
        assert_success(&output);
        assert_eq!(
            fs::read(temp.path().join("archive/nested/file.txt")).unwrap(),
            FILE_CONTENT
        );
    }

    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("renamed.data");
    write_standard_tar(&archive).unwrap();
    let output_path = temp.path().join("explicit-output");
    let output = run_xx(
        temp.path(),
        xx_args!([
            "--format",
            "tar",
            "--output",
            output_path.as_os_str().to_owned(),
            archive.as_os_str().to_owned(),
        ]),
    );
    assert_success(&output);
    assert_eq!(
        fs::read(output_path.join("nested/file.txt")).unwrap(),
        FILE_CONTENT
    );
}

#[test]
fn create_only_refuses_to_change_an_existing_output() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("archive.tar");
    let output_path = temp.path().join("destination");
    write_standard_tar(&archive).unwrap();
    fs::create_dir(&output_path).unwrap();
    fs::write(output_path.join("sentinel"), b"keep me").unwrap();

    let output = run_xx(
        temp.path(),
        xx_args!([
            "--output",
            output_path.as_os_str().to_owned(),
            archive.as_os_str().to_owned(),
        ]),
    );
    assert_exit(&output, 6);
    assert_eq!(fs::read(output_path.join("sentinel")).unwrap(), b"keep me");
    assert!(!output_path.join("nested/file.txt").exists());
}

#[test]
fn rm_leaves_the_existing_output_when_extraction_fails() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("broken.tar");
    let output_path = temp.path().join("destination");
    fs::write(&archive, b"not an archive").unwrap();
    fs::create_dir(&output_path).unwrap();
    fs::write(output_path.join("sentinel"), b"keep me").unwrap();

    let output = run_xx(
        temp.path(),
        xx_args!([
            "--rm",
            "--output",
            output_path.as_os_str().to_owned(),
            archive.as_os_str().to_owned(),
        ]),
    );
    assert_exit(&output, 4);
    assert_eq!(fs::read(output_path.join("sentinel")).unwrap(), b"keep me");
    assert!(!output_path.join("nested/file.txt").exists());
}

#[test]
fn overwrite_merges_conflicts_and_retains_unrelated_entries() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("archive.tar");
    let output_path = temp.path().join("destination");
    write_standard_tar(&archive).unwrap();
    fs::create_dir_all(output_path.join("nested")).unwrap();
    fs::write(output_path.join("nested/file.txt"), b"old content").unwrap();
    fs::write(output_path.join("unrelated"), b"retain this").unwrap();

    let output = run_xx(
        temp.path(),
        xx_args!([
            "--overwrite",
            "--output",
            output_path.as_os_str().to_owned(),
            archive.as_os_str().to_owned(),
        ]),
    );
    assert_success(&output);
    assert_eq!(
        fs::read(output_path.join("nested/file.txt")).unwrap(),
        FILE_CONTENT
    );
    assert_eq!(
        fs::read(output_path.join("unrelated")).unwrap(),
        b"retain this"
    );
    assert_eq!(file_mtime(&output_path.join("nested")), 1_700_000_001);
}

#[test]
fn threads_one_and_many_produce_the_same_zip_tree() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("many.zip");
    let entries = [
        ("bundle/a.txt", b"a".as_slice()),
        ("bundle/nested/b.txt", b"b".as_slice()),
        ("bundle/nested/c.txt", b"c".as_slice()),
        ("bundle/d.txt", b"d".as_slice()),
    ];
    write_zip_with_entries(&archive, &entries).unwrap();

    let one = temp.path().join("one");
    let many = temp.path().join("many");
    let first = run_xx(
        temp.path(),
        xx_args!([
            "--output",
            one.as_os_str().to_owned(),
            "--threads",
            "1",
            archive.as_os_str().to_owned(),
        ]),
    );
    let second = run_xx(
        temp.path(),
        xx_args!([
            "--output",
            many.as_os_str().to_owned(),
            "--threads",
            "4",
            archive.as_os_str().to_owned(),
        ]),
    );
    assert_success(&first);
    assert_success(&second);
    assert_eq!(read_tree(&one), read_tree(&many));
}

#[test]
fn listing_keeps_stdout_data_separate_from_progress() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("archive.zip");
    write_zip_with_entries(
        &archive,
        &[("bundle/file.txt", FILE_CONTENT), ("bundle/other", b"x")],
    )
    .unwrap();

    let output = run_xx(
        temp.path(),
        xx_args!(["--list", "--progress", archive.as_os_str().to_owned()]),
    );
    assert_success(&output);
    assert_eq!(output.stdout, b"bundle/file.txt\nbundle/other\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("phase=list"));
}

#[test]
fn listing_supports_json_and_nul_delimited_names() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("archive.zip");
    write_zip_with_entries(
        &archive,
        &[("bundle/file.txt", FILE_CONTENT), ("bundle/other", b"x")],
    )
    .unwrap();

    let json = run_xx(
        temp.path(),
        xx_args!(["--list", "--json", archive.as_os_str().to_owned()]),
    );
    assert_success(&json);
    let names: Vec<String> = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(names, ["bundle/file.txt", "bundle/other"]);

    let nul = run_xx(
        temp.path(),
        xx_args!(["--list", "-0", archive.as_os_str().to_owned()]),
    );
    assert_success(&nul);
    assert_eq!(nul.stdout, b"bundle/file.txt\0bundle/other\0");
}

#[test]
fn explicit_format_allows_a_renamed_archive_but_unknown_format_without_output_fails() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("archive.data");
    write_standard_tar(&archive).unwrap();

    let missing_output = run_xx(
        temp.path(),
        xx_args!(["--format", "tar", archive.as_os_str().to_owned()]),
    );
    assert_exit(&missing_output, 2);

    let output_path = temp.path().join("renamed-output");
    let explicit = run_xx(
        temp.path(),
        xx_args!([
            "--format",
            "tar",
            "--output",
            output_path.as_os_str().to_owned(),
            archive.as_os_str().to_owned(),
        ]),
    );
    assert_success(&explicit);
}

#[test]
fn a_path_traversal_archive_is_rejected_without_writing_outside_output() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("unsafe.zip");
    write_zip_with_entries(&archive, &[("../outside", b"escaped")]).unwrap();

    let output_path = temp.path().join("destination");
    let output = run_xx(
        temp.path(),
        xx_args!([
            "--output",
            output_path.as_os_str().to_owned(),
            archive.as_os_str().to_owned(),
        ]),
    );
    assert_exit(&output, 5);
    assert!(!temp.path().join("outside").exists());
    assert!(!output_path.exists());
}

#[test]
fn duplicate_tar_names_use_the_last_entry() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("duplicate.tar");
    let mut builder = tar::Builder::new(Vec::new());
    for content in [b"first".as_slice(), b"last".as_slice()] {
        let mut header = tar::Header::new_gnu();
        header.set_path("bundle/value").unwrap();
        header.set_size(content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, content).unwrap();
    }
    fs::write(&archive, builder.into_inner().unwrap()).unwrap();

    let output = run_xx(temp.path(), [archive.as_os_str()]);
    assert_success(&output);
    assert_eq!(
        fs::read(temp.path().join("duplicate/value")).unwrap(),
        b"last"
    );
}

#[test]
fn a_zip_crc_failure_is_rejected_before_publication() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("broken.zip");
    write_zip_with_entries(&archive, &[("bundle/file.txt", b"checksum")]).unwrap();
    let mut bytes = fs::read(&archive).unwrap();
    let position = bytes
        .windows(b"checksum".len())
        .position(|window| window == b"checksum")
        .unwrap();
    bytes[position] ^= 1;
    fs::write(&archive, bytes).unwrap();

    let output = run_xx(temp.path(), [archive.as_os_str()]);
    assert!(!output.status.success());
    assert!(!temp.path().join("broken").exists());
}

#[test]
fn special_file_entries_are_rejected_before_publication() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("special.tar");
    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path("bundle/device").unwrap();
    header.set_entry_type(tar::EntryType::Fifo);
    header.set_mode(0o644);
    header.set_size(0);
    header.set_cksum();
    builder.append(&header, std::io::empty()).unwrap();
    fs::write(&archive, builder.into_inner().unwrap()).unwrap();

    let output = run_xx(temp.path(), [archive.as_os_str()]);
    assert_exit(&output, 4);
    assert!(!temp.path().join("special").exists());
}

#[cfg(unix)]
#[test]
fn an_existing_destination_symlink_cannot_redirect_an_overwrite() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("archive.tar");
    let outside = temp.path().join("outside");
    let output_path = temp.path().join("destination");
    write_standard_tar(&archive).unwrap();
    fs::create_dir(&outside).unwrap();
    symlink(&outside, &output_path).unwrap();

    let output = run_xx(
        temp.path(),
        xx_args!([
            "--overwrite",
            "--output",
            output_path.as_os_str().to_owned(),
            archive.as_os_str().to_owned(),
        ]),
    );
    assert_exit(&output, 6);
    assert!(!outside.join("nested/file.txt").exists());
}

#[test]
fn archive_listing_and_extraction_use_the_expected_logical_root() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("archive.tar");
    write_standard_tar(&archive).unwrap();
    let output_path = temp.path().join("destination");

    let output = run_xx(
        temp.path(),
        xx_args!([
            "--output",
            output_path.as_os_str().to_owned(),
            archive.as_os_str().to_owned(),
        ]),
    );
    assert_success(&output);
    assert!(output_path.join("nested/file.txt").exists());
    assert!(!output_path.join(ROOT).exists());
}
