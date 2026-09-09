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
    x86_64)
        deb_arch=amd64
        package_arch=x86_64
        ;;
    aarch64)
        deb_arch=arm64
        package_arch=aarch64
        ;;
    *)
        echo "unsupported architecture: $target_arch (Velvet Scroll supports x86_64 and aarch64)" >&2
        exit 1
        ;;
esac

if printf '%s\n' "$PACKAGE_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    deb_version=$PACKAGE_VERSION
    rpm_version=$PACKAGE_VERSION
    rpm_release=1
    pkgver=$PACKAGE_VERSION
elif printf '%s\n' "$PACKAGE_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+-dev\.[0-9]+\.g[0-9a-f]+$'; then
    base_version=${PACKAGE_VERSION%%-*}
    prerelease=${PACKAGE_VERSION#*-}
    deb_version=$base_version~$prerelease
    rpm_version=$base_version
    rpm_release=0.$prerelease
    pkgver=$base_version$prerelease
else
    echo "unsupported package version: $PACKAGE_VERSION" >&2
    echo "expected X.Y.Z or X.Y.Z-dev.N.gSHA" >&2
    exit 1
fi

"$project_dir/scripts/build-deb.sh"
"$project_dir/scripts/build-rpm.sh"
"$project_dir/scripts/build-arch.sh"
"$project_dir/scripts/build-archive.sh"

deb_package=$project_dir/dist/velvet-scroll_${deb_version}_${deb_arch}.deb
rpm_package=$project_dir/dist/velvet-scroll-$rpm_version-$rpm_release.$package_arch.rpm
arch_package=$project_dir/dist/velvet-scroll-$pkgver-1-$package_arch.pkg.tar.zst
archive=$project_dir/dist/velvet-scroll-$PACKAGE_VERSION-linux-$package_arch.tar.gz

for artifact in "$deb_package" "$rpm_package" "$arch_package" "$archive"; do
    if [ ! -f "$artifact" ]; then
        echo "package builder did not create expected artifact: $artifact" >&2
        exit 1
    fi
done

echo "Built all packages for $package_arch"
