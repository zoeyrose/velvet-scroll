#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
test_root=$(mktemp -d "${TMPDIR:-/tmp}/velvet-scroll-version-test.XXXXXX")
trap 'rm -rf -- "$test_root"' EXIT HUP INT TERM
fixture=$test_root/repo
mkdir -p "$fixture/scripts"
cp "$project_dir/scripts/version.sh" "$fixture/scripts/version.sh"

git -C "$fixture" init -q
git -C "$fixture" config user.name "Version Test"
git -C "$fixture" config user.email "version-test@example.invalid"
printf 'first\n' >"$fixture/content"
git -C "$fixture" add content scripts/version.sh
git -C "$fixture" commit -q -m first

short_commit=$(git -C "$fixture" rev-parse --short=12 HEAD)
actual=$("$fixture/scripts/version.sh")
expected=0.0.0-dev.1.g$short_commit
test "$actual" = "$expected"

test "$(PACKAGE_VERSION=2.4.6 "$fixture/scripts/version.sh")" = 2.4.6
test "$(PACKAGE_VERSION=2.4.7-dev.19.gabcdef0 "$fixture/scripts/version.sh")" = 2.4.7-dev.19.gabcdef0
if PACKAGE_VERSION=2.4 "$fixture/scripts/version.sh" >/dev/null 2>&1; then
    echo "invalid PACKAGE_VERSION was accepted" >&2
    exit 1
fi
if PACKAGE_VERSION=02.4.6 "$fixture/scripts/version.sh" >/dev/null 2>&1; then
    echo "version with a leading zero was accepted" >&2
    exit 1
fi
invalid_multiline='2.4.6
3.0.0'
if PACKAGE_VERSION=$invalid_multiline "$fixture/scripts/version.sh" >/dev/null 2>&1; then
    echo "multiline version was accepted" >&2
    exit 1
fi

git -C "$fixture" tag v1.2.3
test "$("$fixture/scripts/version.sh")" = 1.2.3
printf 'second\n' >>"$fixture/content"
git -C "$fixture" commit -q -am second
short_commit=$(git -C "$fixture" rev-parse --short=12 HEAD)
test "$("$fixture/scripts/version.sh")" = 1.2.3-dev.2.g$short_commit

outside_git=$test_root/source-archive
mkdir -p "$outside_git/scripts"
cp "$project_dir/scripts/version.sh" "$outside_git/scripts/version.sh"
if "$outside_git/scripts/version.sh" >/dev/null 2>&1; then
    echo "source archive without PACKAGE_VERSION was accepted" >&2
    exit 1
fi
test "$(PACKAGE_VERSION=3.0.0 "$outside_git/scripts/version.sh")" = 3.0.0

echo "Version resolver tests passed"
