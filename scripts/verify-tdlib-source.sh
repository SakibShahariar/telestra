#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../packaging/tdlib/source.env
source "$script_dir/../packaging/tdlib/source.env"

if [[ $# -ne 1 ]]; then
    printf 'Usage: %s PATH_TO_TDLIB_CHECKOUT\n' "$0" >&2
    exit 2
fi

source_dir="$1"
if [[ ! -d "$source_dir" ]]; then
    printf 'TDLib source directory does not exist: %s\n' "$source_dir" >&2
    exit 2
fi

actual_commit="$(git -C "$source_dir" rev-parse --verify HEAD 2>/dev/null)" || {
    printf 'Not a Git checkout: %s\n' "$source_dir" >&2
    exit 2
}

if [[ "$actual_commit" != "$TDLIB_GIT_COMMIT" ]]; then
    printf 'TDLib commit mismatch: expected %s, found %s\n' \
        "$TDLIB_GIT_COMMIT" "$actual_commit" >&2
    exit 1
fi

if [[ -n "$(git -C "$source_dir" status --porcelain --untracked-files=all)" ]]; then
    printf 'TDLib checkout is not clean; refusing to verify a modified source tree.\n' >&2
    exit 1
fi

printf 'Verified TDLib %s at %s\n' "$TDLIB_VERSION" "$TDLIB_GIT_COMMIT"
