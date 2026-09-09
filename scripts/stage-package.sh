#!/bin/sh
set -eu

if [ "$#" -gt 2 ]; then
    echo "usage: $0 [STAGE_DIR] [PREFIX]" >&2
    exit 2
fi

project_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
stage_dir=${1:-"$project_dir/dist/stage"}
prefix=${2:-/usr}

case "$stage_dir" in
    /|""|.)
        echo "refusing unsafe staging directory: $stage_dir" >&2
        exit 2
        ;;
esac

mkdir -p "$stage_dir"
make -C "$project_dir" install install-udev install-uinput-module DESTDIR="$stage_dir" PREFIX="$prefix"

echo "Package tree staged at $stage_dir"
