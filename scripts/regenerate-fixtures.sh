#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
fixtures="$root/tests/fixtures"
work="$root/tests/.fixture-work"

command -v python3 >/dev/null 2>&1 || {
    echo "regenerate-fixtures: python3 is required" >&2
    exit 1
}
command -v tar >/dev/null 2>&1 || {
    echo "regenerate-fixtures: tar is required" >&2
    exit 1
}
compress_command=$(command -v compress || true)
if [ -z "$compress_command" ]; then
    echo "regenerate-fixtures: compress is required" >&2
    exit 1
fi
seven_zip_command=$(command -v 7zz || command -v 7z || true)
if [ -z "$seven_zip_command" ]; then
    echo "regenerate-fixtures: 7zz or 7z is required" >&2
    exit 1
fi

cleanup() {
    rm -rf "$work"
}
trap cleanup EXIT HUP INT TERM

rm -rf "$work"
mkdir -p "$work/bundle/nested" "$work/bundle/bin"
printf '%s\n' 'xx external fixture' > "$work/bundle/nested/file.txt"
printf '%s\n' '#!/bin/sh' 'echo fixture' > "$work/bundle/bin/run.sh"
chmod 755 "$work/bundle/bin/run.sh"

python3 - "$work/bundle/nested/payload.bin" <<'PY'
from pathlib import Path
import sys

state = 0x12345678
output = bytearray()
for _ in range(131072):
    state = (state * 1664525 + 1013904223) & 0xffffffff
    output.append((state >> 24) & 0xff)
Path(sys.argv[1]).write_bytes(output)
PY

COPYFILE_DISABLE=1 tar -cf "$work/reset.tar" -C "$work" bundle
"$compress_command" -c "$work/reset.tar" > "$fixtures/external-reset.tar.Z"
rm "$work/bundle/nested/payload.bin"
rm -f "$fixtures/external.7z"
(
    cd "$work"
    "$seven_zip_command" a -t7z -mx=1 "$fixtures/external.7z" bundle
)
