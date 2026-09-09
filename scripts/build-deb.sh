#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$project_dir/Cargo.toml" | head -n 1)
target_arch=$(rustc -vV | sed -n 's/^host: //p' | cut -d- -f1)

case "$target_arch" in
    x86_64) deb_arch=amd64 ;;
    aarch64) deb_arch=arm64 ;;
    *)
        echo "unsupported architecture: $target_arch (Velvet Scroll supports x86_64 and aarch64)" >&2
        exit 1
        ;;
esac

for tool in dpkg-deb dpkg-shlibdeps; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "$tool is required to build the Debian package (install dpkg-dev)" >&2
        exit 1
    }
done

package_root="$project_dir/dist/deb-root"
output="$project_dir/dist/velvet-scroll_${version}_${deb_arch}.deb"

case "$package_root" in
    "$project_dir"/dist/*) ;;
    *)
        echo "refusing unsafe package root: $package_root" >&2
        exit 2
        ;;
esac

rm -rf "$package_root"
mkdir -p "$package_root/DEBIAN" "$project_dir/dist"
"$project_dir/scripts/stage-package.sh" "$package_root" /usr

shlibs_output=$(
    cd "$project_dir/packaging"
    dpkg-shlibdeps -O -e"$package_root/usr/bin/velvet-scroll"
)
shlibs_depends=${shlibs_output#shlibs:Depends=}
if [ -z "$shlibs_depends" ] || [ "$shlibs_depends" = "$shlibs_output" ]; then
    echo "dpkg-shlibdeps did not produce runtime dependencies" >&2
    exit 1
fi

sed \
    -e "s/@VERSION@/$version/g" \
    -e "s/@ARCH@/$deb_arch/g" \
    -e "s|@SHLIBS_DEPENDS@|$shlibs_depends|g" \
    "$project_dir/packaging/debian/control.in" >"$package_root/DEBIAN/control"

dpkg-deb --root-owner-group --build "$package_root" "$output"
echo "Built $output"
