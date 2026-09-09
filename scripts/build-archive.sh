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

for tool in tar gzip; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "$tool is required to build the portable archive" >&2
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
work_dir=$(mktemp -d "$project_dir/dist/.archive-build.XXXXXX")
bundle_name=velvet-scroll-$PACKAGE_VERSION-linux-$package_arch
bundle_dir=$work_dir/$bundle_name
output=$project_dir/dist/$bundle_name.tar.gz
temporary_output=$output.tmp
trap 'rm -rf -- "$work_dir"; rm -f -- "$temporary_output"' EXIT HUP INT TERM
mkdir -p "$bundle_dir/files"

"$project_dir/scripts/stage-package.sh" "$bundle_dir/files" /usr

(
    cd "$bundle_dir/files"
    find . -type f -print | sed 's|^\./||' | LC_ALL=C sort
) >"$bundle_dir/MANIFEST.files"
(
    cd "$bundle_dir/files"
    find . -mindepth 1 -type d -print | sed 's|^\./||' | LC_ALL=C sort -r
) >"$bundle_dir/MANIFEST.dirs"

cat >"$bundle_dir/install.sh" <<'INSTALL_SCRIPT'
#!/bin/sh
set -eu

bundle_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
install_root=${DESTDIR:-/}
case "$install_root" in
    /*) ;;
    *)
        echo "DESTDIR must be an absolute path" >&2
        exit 2
        ;;
esac
if [ "$install_root" = / ] && [ "$(id -u)" -ne 0 ]; then
    echo "Installing host files under / requires root; run sudo ./install.sh" >&2
    exit 1
fi

mkdir -p "$install_root"
cp -R "$bundle_dir/files/." "$install_root/"

echo "Velvet Scroll is installed. To apply device access now, run:"
echo "  sudo udevadm control --reload-rules"
echo "  sudo modprobe uinput"
echo "Then start the user service with:"
echo "  systemctl --user daemon-reload"
echo "  systemctl --user start velvet-scroll"
INSTALL_SCRIPT

cat >"$bundle_dir/uninstall.sh" <<'UNINSTALL_SCRIPT'
#!/bin/sh
set -eu

bundle_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
install_root=${DESTDIR:-/}
case "$install_root" in
    /*) ;;
    *)
        echo "DESTDIR must be an absolute path" >&2
        exit 2
        ;;
esac
if [ "$install_root" = / ] && [ "$(id -u)" -ne 0 ]; then
    echo "Removing host files under / requires root; run sudo ./uninstall.sh" >&2
    exit 1
fi

while IFS= read -r relative_path; do
    case "$relative_path" in
        ''|/*|..|../*|*/..|*/../*)
            echo "unsafe path in MANIFEST.files: $relative_path" >&2
            exit 1
            ;;
    esac
    rm -f -- "$install_root/$relative_path"
done <"$bundle_dir/MANIFEST.files"

while IFS= read -r relative_path; do
    case "$relative_path" in
        ''|/*|..|../*|*/..|*/../*)
            echo "unsafe path in MANIFEST.dirs: $relative_path" >&2
            exit 1
            ;;
    esac
    rmdir -- "$install_root/$relative_path" 2>/dev/null || true
done <"$bundle_dir/MANIFEST.dirs"

echo "Velvet Scroll was removed. Reload host integration with:"
echo "  sudo udevadm control --reload-rules"
echo "  systemctl --user daemon-reload"
UNINSTALL_SCRIPT

cat >"$bundle_dir/README.txt" <<'BUNDLE_README'
Velvet Scroll native Linux bundle

This bundle contains the Velvet Scroll binary, GUI, desktop integration,
documentation, systemd user service, udev device-access rule, and uinput
modules-load configuration. The udev rule grants the active local seat user
access to selected mouse event devices and /dev/uinput. Review the installed
documentation in files/usr/share/doc/velvet-scroll before installation.

The bundle requires a glibc-based Linux system plus Python 3, PyQt6, and Qt's
SVG support from the host distribution. Its filename identifies the native CPU
architecture it supports.

Install on the host:
  sudo ./install.sh

Test installation into a temporary root without changing the host:
  DESTDIR=/absolute/path/to/root ./install.sh

Remove the same files later:
  systemctl --user disable --now velvet-scroll
  sudo ./uninstall.sh
BUNDLE_README

chmod 0755 "$bundle_dir/install.sh" "$bundle_dir/uninstall.sh"
chmod 0644 "$bundle_dir/MANIFEST.files" "$bundle_dir/MANIFEST.dirs" "$bundle_dir/README.txt"
find "$bundle_dir" -type d -exec chmod 0755 {} +

rm -f -- "$temporary_output"
tar \
    --format=ustar \
    --sort=name \
    --mtime="@$build_date" \
    --owner=0 \
    --group=0 \
    --numeric-owner \
    -C "$work_dir" \
    -cf "$work_dir/$bundle_name.tar" "$bundle_name"
gzip -n -9 <"$work_dir/$bundle_name.tar" >"$temporary_output"
mv -f -- "$temporary_output" "$output"
echo "Built $output"
