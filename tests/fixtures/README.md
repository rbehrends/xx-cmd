# Integration fixtures

These fixtures are used by the process-level tests in `tests/cli.rs`. Normal
`cargo test` runs do not require any archive command to be installed.

- `external-reset.tar.Z` was produced by Unix `compress` from a small tar
  archive containing a deterministic 128 KiB pseudo-random payload. The size
  is intentional: the stream crosses a dictionary reset and exercises the
  block-mode `.Z` reader against an external producer.
- `external.7z` was produced by 7-Zip with the lowest compression level. It is
  kept as a reader/interoperability fixture because the current 7z dependency
  configuration does not enable its compression API.

The fixtures can be regenerated with:

```console
$ just regenerate-fixtures
```

That optional recipe requires `python3`, `tar`, `compress`, and `7zz` (or
`7z`). Regeneration is a maintainer operation; it is deliberately not part of
`just test` or `just verify`.
