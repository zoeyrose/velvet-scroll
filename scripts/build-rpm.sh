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
    x86_64|aarch64) rpm_arch=$target_arch ;;
    *)
        echo "unsupported architecture: $target_arch (Velvet Scroll supports x86_64 and aarch64)" >&2
        exit 1
        ;;
esac

if printf '%s\n' "$PACKAGE_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
    rpm_version=$PACKAGE_VERSION
    rpm_release=1
elif printf '%s\n' "$PACKAGE_VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+-dev\.[0-9]+\.g[0-9a-f]+$'; then
    rpm_version=${PACKAGE_VERSION%%-*}
    rpm_release=0.${PACKAGE_VERSION#*-}
else
    echo "unsupported package version: $PACKAGE_VERSION" >&2
    echo "expected X.Y.Z or X.Y.Z-dev.N.gSHA" >&2
    exit 1
fi

for tool in rpmbuild rpm; do
    command -v "$tool" >/dev/null 2>&1 || {
        echo "$tool is required to build the RPM package" >&2
        exit 1
    }
done

mkdir -p "$project_dir/dist"
work_dir=$(mktemp -d "$project_dir/dist/.rpm-build.XXXXXX")
trap 'rm -rf -- "$work_dir"' EXIT HUP INT TERM
stage_dir=$work_dir/stage
top_dir=$work_dir/rpmbuild
mkdir -p "$stage_dir" "$top_dir/BUILD" "$top_dir/BUILDROOT" "$top_dir/RPMS" "$top_dir/SOURCES" "$top_dir/SPECS" "$top_dir/SRPMS"

"$project_dir/scripts/stage-package.sh" "$stage_dir" /usr

rpmbuild -bb \
    --define "_topdir $top_dir" \
    --define "_target_cpu $rpm_arch" \
    --define "velvet_scroll_stage $stage_dir" \
    --define "velvet_scroll_version $rpm_version" \
    --define "velvet_scroll_release $rpm_release" \
    "$project_dir/packaging/rpm/velvet-scroll.spec"

built_rpm=$top_dir/RPMS/$rpm_arch/velvet-scroll-$rpm_version-$rpm_release.$rpm_arch.rpm
if [ ! -f "$built_rpm" ]; then
    echo "rpmbuild did not create the expected package: $built_rpm" >&2
    exit 1
fi

reported_nevra=$(rpm -qp --queryformat '%{VERSION} %{RELEASE} %{ARCH}\n' "$built_rpm")
if [ "$reported_nevra" != "$rpm_version $rpm_release $rpm_arch" ]; then
    echo "unexpected RPM version or architecture: $reported_nevra" >&2
    exit 1
fi
if ! rpm -qp --requires "$built_rpm" | grep -F 'libc.so.6' >/dev/null; then
    echo "RPM automatic dependency generation did not find the binary's glibc dependency" >&2
    exit 1
fi

output=$project_dir/dist/velvet-scroll-$rpm_version-$rpm_release.$rpm_arch.rpm
temporary_output=$output.tmp
trap 'rm -rf -- "$work_dir"; rm -f -- "$temporary_output"' EXIT HUP INT TERM
install -m 0644 "$built_rpm" "$temporary_output"
mv -f -- "$temporary_output" "$output"
echo "Built $output"
