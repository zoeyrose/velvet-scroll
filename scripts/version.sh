#!/bin/sh
set -eu

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

is_stable_version() {
    case "$1" in
        *'
'*) return 1 ;;
    esac
    printf '%s\n' "$1" | grep -Eq '^((0|[1-9][0-9]*)\.){2}(0|[1-9][0-9]*)$'
}

validate_version() {
    case "$1" in
        *'
'*) valid=false ;;
        *)
            if printf '%s\n' "$1" | grep -Eq '^((0|[1-9][0-9]*)\.){2}(0|[1-9][0-9]*)(-dev\.(0|[1-9][0-9]*)\.g[0-9a-f]{7,40})?$'; then
                valid=true
            else
                valid=false
            fi
            ;;
    esac
    if [ "$valid" != true ]; then
        echo "invalid package version: $1" >&2
        echo "expected X.Y.Z or X.Y.Z-dev.N.gSHA (7-40 lowercase hex digits)" >&2
        return 1
    fi
}

if [ "${PACKAGE_VERSION+x}" = x ]; then
    validate_version "$PACKAGE_VERSION"
    printf '%s\n' "$PACKAGE_VERSION"
    exit 0
fi

if ! git -C "$project_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    echo "cannot derive a version outside a Git checkout; set PACKAGE_VERSION" >&2
    exit 1
fi

head_commit=$(git -C "$project_dir" rev-parse --verify HEAD)
exact_version=
for tag in $(git -C "$project_dir" tag --points-at "$head_commit"); do
    case "$tag" in
        v*) candidate=${tag#v} ;;
        *) continue ;;
    esac
    if is_stable_version "$candidate"; then
        if [ -n "$exact_version" ] && [ "$candidate" != "$exact_version" ]; then
            echo "multiple stable version tags point at HEAD" >&2
            exit 1
        fi
        exact_version=$candidate
    fi
done
if [ -n "$exact_version" ]; then
    validate_version "$exact_version"
    printf '%s\n' "$exact_version"
    exit 0
fi

nearest_tag=$(git -C "$project_dir" describe --tags --abbrev=0 --match 'v[0-9]*.[0-9]*.[0-9]*' "$head_commit" 2>/dev/null || true)
if [ -n "$nearest_tag" ]; then
    base_version=${nearest_tag#v}
    if ! is_stable_version "$base_version"; then
        echo "nearest version tag is invalid: $nearest_tag" >&2
        exit 1
    fi
else
    base_version=0.0.0
fi
commit_count=$(git -C "$project_dir" rev-list --count "$head_commit")
short_commit=$(git -C "$project_dir" rev-parse --short=12 "$head_commit")
resolved_version=$base_version-dev.$commit_count.g$short_commit
validate_version "$resolved_version"
printf '%s\n' "$resolved_version"
