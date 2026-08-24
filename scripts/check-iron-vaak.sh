#!/usr/bin/env bash
set -euo pipefail

repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_dir"

cargo fmt --manifest-path crates/iron-vaak-ffi/Cargo.toml -- --check
cargo test --release --locked --manifest-path crates/iron-vaak-ffi/Cargo.toml
cargo fmt --manifest-path crates/iron-vaak-native/Cargo.toml -- --check
cargo test --release --locked --manifest-path crates/iron-vaak-native/Cargo.toml

gcc -std=c11 -fsyntax-only crates/iron-vaak-ffi/tests/header_smoke.c
g++ -std=c++17 -fsyntax-only crates/iron-vaak-ffi/tests/header_smoke.c

host_target="$(rustc -vV | sed -n 's/^host: //p')"
scripts/stage-iron-vaak-native.sh "$host_target"

dotnet build dotnet/src/IronVaak/IronVaak.csproj -c Release
dotnet pack dotnet/src/IronVaak/IronVaak.csproj -c Release -o /tmp/iron-vaak-pack

case "$host_target" in
    x86_64-unknown-linux-gnu|aarch64-unknown-linux-gnu)
        native_dir="$repo_dir/crates/iron-vaak-native/target/$host_target/release"
        expected_symbols="$(sed -n 's/^IRON_VAAK_API uint32_t \(iron_vaak_v0_[a-z_]*\)(.*/\1/p' \
            crates/iron-vaak-ffi/include/iron_vaak_v0.h | LC_ALL=C sort)"
        actual_symbols="$(nm -D --defined-only "$native_dir/libiron_vaak_native.so" | \
            awk '{print $3}' | LC_ALL=C sort)"
        if [[ "$actual_symbols" != "$expected_symbols" ]]; then
            echo "native dynamic exports differ from the public C header" >&2
            diff -u <(printf '%s\n' "$expected_symbols") <(printf '%s\n' "$actual_symbols") || true
            exit 1
        fi
        LD_LIBRARY_PATH="$native_dir${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
            dotnet run --project dotnet/tests/IronVaak.Smoke/IronVaak.Smoke.csproj -c Release
        ;;
    x86_64-apple-darwin|aarch64-apple-darwin)
        native_dir="$repo_dir/crates/iron-vaak-native/target/$host_target/release"
        DYLD_LIBRARY_PATH="$native_dir${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}" \
            dotnet run --project dotnet/tests/IronVaak.Smoke/IronVaak.Smoke.csproj -c Release
        ;;
    *)
        echo "managed runtime smoke is not scripted for host target $host_target" >&2
        ;;
esac

if [[ "${IRON_VAAK_CHECK_AOT:-0}" == "1" && "$host_target" == "x86_64-unknown-linux-gnu" ]]; then
    dotnet publish dotnet/tests/IronVaak.Smoke/IronVaak.Smoke.csproj \
        -c Release -r linux-x64 -p:PublishAot=true -p:TargetFrameworks=net8.0 \
        -o /tmp/iron-vaak-aot
    LD_LIBRARY_PATH="$repo_dir/crates/iron-vaak-native/target/$host_target/release" \
        /tmp/iron-vaak-aot/IronVaak.Smoke
fi
