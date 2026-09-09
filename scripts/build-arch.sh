#!/bin/sh
set -eu

if [ "$#" -ne 0 ]; then
    echo "usage: $0" >&2
    exit 2
fi

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
PACKAGE_VERSION=$("$project_dir/scripts/version.sh")
export PACKAGE_VERSION
target_arch=$(uname -m)

case "$target_arch" in
    x86_64|aarch64) package_arch=$target_arch ;;
    *)
        echo "unsupported architecture: $target_arch (Velvet Scroll supports x86_64 and aarch64)" >&2
        exit 1
        ;;
esac

if ! printf '%s\n' "$PACKAGE_VERSION" | grep -Eq '^([0-9]+\.[0-9]+\.[0-9]+|[0-9]+\.[0-9]+\.[0-9]+-dev\.[0-9]+\.g[0-9a-f]+)$'; then
    echo "unsupported package version: $PACKAGE_VERSION" >&2
    echo "expected X.Y.Z or X.Y.Z-dev.N.gSHA" >&2
    exit 1
fi

# Arch pkgver values cannot contain hyphens. Joining the prerelease marker to
# the base (X.Y.Zdev.N.gSHA) is accepted by pacman and sorts before X.Y.Z.
case "$PACKAGE_VERSION" in
    *-dev.*) pkgver=${PACKAGE_VERSION%%-*}${PACKAGE_VERSION#*-} ;;
    *) pkgver=$PACKAGE_VERSION ;;
esac
pkgrel=1

for tool in tar zstd; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "$tool is required to build the Arch Linux package" >&2
        exit 1
    }
done

if [ -n "${SOURCE_DATE_EPOCH:-}" ]; then
    build_date=$SOURCE_DATE_EPOCH
else
    build_date=$(git -C "$project_dir" log -1 --format=%ct 2>/dev/null || date +%s)
fi
case "$build_date" in
    ''|*[!0-9]*)
        echo "SOURCE_DATE_EPOCH must be an integer Unix timestamp" >&2
        exit 1
        ;;
esac

mkdir -p "$project_dir/dist"
work_dir=$(mktemp -d "$project_dir/dist/.arch-build.XXXXXX")
trap 'rm -rf -- "$work_dir"' EXIT HUP INT TERM
package_root=$work_dir/root
mkdir -p "$package_root"

"$project_dir/scripts/stage-package.sh" "$package_root" /usr
installed_size=$(du -sb "$package_root" | awk '{print $1}')

sed \
    -e "s/@PKGVER@/$pkgver/g" \
    -e "s/@PKGREL@/$pkgrel/g" \
    -e "s/@BUILDDATE@/$build_date/g" \
    -e "s/@SIZE@/$installed_size/g" \
    -e "s/@ARCH@/$package_arch/g" \
    "$project_dir/packaging/arch/PKGINFO.in" >"$package_root/.PKGINFO"
chmod 0644 "$package_root/.PKGINFO"
find "$package_root" -type d -exec chmod 0755 {} +

output=$project_dir/dist/velvet-scroll-$pkgver-$pkgrel-$package_arch.pkg.tar.zst
temporary_output=$output.tmp
temporary_tar=$work_dir/velvet-scroll.pkg.tar
trap 'rm -rf -- "$work_dir"; rm -f -- "$temporary_output"' EXIT HUP INT TERM
rm -f -- "$temporary_output"
tar \
    --format=ustar \
    --sort=name \
    --mtime="@$build_date" \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    -C "$package_root" \
    -cf "$temporary_tar" .PKGINFO etc usr
zstd -q -19 -T0 "$temporary_tar" -o "$temporary_output"
mv -f -- "$temporary_output" "$output"
echo "Built $output"
