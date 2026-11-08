#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "$script_dir/.." && pwd)"
# shellcheck source=../packaging/tdlib/source.env
source "$repo_root/packaging/tdlib/source.env"

source_dir="${1:-$repo_root/target/tdlib-source}"
prefix="${2:-$repo_root/target/tdlib}"
build_dir="${3:-$repo_root/target/tdlib-build}"
jobs="${TDLIB_BUILD_JOBS:-2}"

if ! [[ "$jobs" =~ ^[1-9][0-9]*$ ]]; then
    printf 'TDLIB_BUILD_JOBS must be a positive integer.\n' >&2
    exit 2
fi

if [[ ! -d "$source_dir/.git" ]]; then
    if [[ -e "$source_dir" ]]; then
        printf 'TDLib source path exists but is not a Git checkout: %s\n' "$source_dir" >&2
        exit 2
    fi
    mkdir -p "$(dirname -- "$source_dir")"
    git clone --filter=blob:none --no-checkout "$TDLIB_GIT_URL" "$source_dir"
    git -C "$source_dir" fetch --depth 1 origin "$TDLIB_GIT_COMMIT"
    git -C "$source_dir" checkout --detach "$TDLIB_GIT_COMMIT"
fi

"$script_dir/verify-tdlib-source.sh" "$source_dir"

cmake -S "$source_dir" -B "$build_dir" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$prefix" \
    -DTD_INSTALL_STATIC_LIBRARIES=OFF \
    -DTD_INSTALL_SHARED_LIBRARIES=ON \
    -DTD_ENABLE_JNI=OFF \
    -DTD_ENABLE_DOTNET=OFF \
    -DBUILD_TESTING=OFF

cmake --build "$build_dir" --target tdjson --parallel "$jobs"
cmake --install "$build_dir"

library="$(find "$prefix" -type f -name 'libtdjson.so*' -print -quit)"
if [[ -z "$library" ]]; then
    printf 'TDLib build did not install libtdjson.so under %s\n' "$prefix" >&2
    exit 1
fi

for symbol in td_create_client_id td_send td_receive td_execute; do
    if ! nm -D --defined-only "$library" | awk '{print $NF}' | grep -Fxq "$symbol"; then
        printf 'TDLib library is missing required symbol: %s\n' "$symbol" >&2
        exit 1
    fi
done

dependencies="$(ldd "$library")"
if grep -Fq 'not found' <<<"$dependencies"; then
    printf 'TDLib library has unresolved runtime dependencies:\n%s\n' "$dependencies" >&2
    exit 1
fi

printf 'Built TDLib %s for %s\n' "$TDLIB_VERSION" "$(uname -m)"
printf 'Library: %s\n' "$library"
