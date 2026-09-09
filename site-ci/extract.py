"""Extract untrusted ZIP entries into a fresh directory as static files only."""

import pathlib
import re
import stat
import sys
import zipfile

MAX_FILES = 1000
MAX_FILE_SIZE = 10 * 1024 * 1024
MAX_TOTAL_SIZE = 50 * 1024 * 1024
EXTENSIONS = {'.html', '.css', '.js', '.json', '.svg', '.png', '.jpg', '.jpeg', '.webp', '.avif', '.gif', '.ico', '.woff', '.woff2', '.txt', '.xml', '.webmanifest'}
RESERVED = {'functions', 'node_modules', '_worker.js', '_routes.json', '_redirects', 'wrangler.toml', 'wrangler.json', 'wrangler.jsonc', 'package.json', 'package-lock.json'}
HEADERS = """/*
  Content-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src 'self'; connect-src 'self' https://api.github.com; object-src 'none'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'
  X-Content-Type-Options: nosniff
  Referrer-Policy: no-referrer
  Permissions-Policy: camera=(), microphone=(), geolocation=()
  X-Frame-Options: DENY
"""


def extract_static(archive_path, destination):
    destination = pathlib.Path(destination)
    if destination.exists():
        raise ValueError('Extraction destination must be new')
    with zipfile.ZipFile(archive_path) as archive:
        entries = archive.infolist()
        if len(entries) > MAX_FILES:
            raise ValueError('Too many artifact entries')
        seen = set()
        total = 0
        approved = []
        for entry in entries:
            name = entry.filename
            parts = name.rstrip('/').split('/')
            if not name or '\\' in name or any(not re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.-]*', part) or part in {'.', '..'} for part in parts):
                raise ValueError('Unsafe artifact path')
            normalized = '/'.join(parts).lower()
            if normalized in seen:
                raise ValueError('Duplicate artifact path')
            seen.add(normalized)
            mode = entry.external_attr >> 16
            if stat.S_ISLNK(mode) or (stat.S_IFMT(mode) and not (stat.S_ISREG(mode) or stat.S_ISDIR(mode))):
                raise ValueError('Artifact contains a link or special file')
            if any(part.lower() in RESERVED for part in parts):
                raise ValueError('Artifact contains executable or deployment configuration')
            if entry.is_dir():
                continue
            total += entry.file_size
            if entry.flag_bits & 1 or entry.file_size > MAX_FILE_SIZE or total > MAX_TOTAL_SIZE:
                raise ValueError('Artifact is encrypted or exceeds size limits')
            # Build-provided headers are data, and never define deployed policy.
            if normalized == '_headers':
                continue
            if pathlib.PurePosixPath(name).suffix.lower() not in EXTENSIONS:
                raise ValueError('Artifact file type is not allowed')
            approved.append(entry)
        if not any(entry.filename == 'index.html' for entry in approved):
            raise ValueError('Artifact must contain index.html')
        destination.mkdir(parents=True)
        for entry in approved:
            target = destination.joinpath(*entry.filename.split('/'))
            target.parent.mkdir(parents=True, exist_ok=True)
            data = archive.read(entry)  # CRC validation occurs before writing.
            if len(data) != entry.file_size:
                raise ValueError('Artifact entry size mismatch')
            with target.open('xb') as output:
                output.write(data)
        (destination / '_headers').write_text(HEADERS, encoding='utf-8')
    return len(approved)


if __name__ == '__main__':
    if len(sys.argv) != 3:
        raise SystemExit('Usage: extract.py ARCHIVE NEW_DESTINATION')
    print(f'Validated {extract_static(sys.argv[1], sys.argv[2])} static files')
