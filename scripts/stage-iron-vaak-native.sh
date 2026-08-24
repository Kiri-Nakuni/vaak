#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <rust-target>" >&2
    exit 2
fi

target="$1"
repo_dir="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$repo_dir/crates/iron-vaak-native/Cargo.toml"
artifact_dir="$repo_dir/crates/iron-vaak-native/target/$target/release"
unity_root="$repo_dir/packages/com.kiri-nakuni.iron-vaak/Runtime/Plugins"
nuget_root="$repo_dir/dotnet/native/runtimes"

cargo build --release --locked --manifest-path "$manifest" --target "$target"

case "$target" in
    x86_64-unknown-linux-gnu)
        source_file="$artifact_dir/libiron_vaak_native.so"
        unity_file="$unity_root/Linux/x86_64/libiron_vaak_native.so"
        nuget_file="$nuget_root/linux-x64/native/libiron_vaak_native.so"
        ;;
    aarch64-unknown-linux-gnu)
        source_file="$artifact_dir/libiron_vaak_native.so"
        unity_file="$unity_root/Linux/ARM64/libiron_vaak_native.so"
        nuget_file="$nuget_root/linux-arm64/native/libiron_vaak_native.so"
        ;;
    x86_64-pc-windows-msvc)
        source_file="$artifact_dir/iron_vaak_native.dll"
        unity_file="$unity_root/Windows/x86_64/iron_vaak_native.dll"
        nuget_file="$nuget_root/win-x64/native/iron_vaak_native.dll"
        ;;
    aarch64-pc-windows-msvc)
        source_file="$artifact_dir/iron_vaak_native.dll"
        unity_file="$unity_root/Windows/ARM64/iron_vaak_native.dll"
        nuget_file="$nuget_root/win-arm64/native/iron_vaak_native.dll"
        ;;
    x86_64-apple-darwin)
        source_file="$artifact_dir/libiron_vaak_native.dylib"
        unity_file="$unity_root/macOS/x86_64/libiron_vaak_native.dylib"
        nuget_file="$nuget_root/osx-x64/native/libiron_vaak_native.dylib"
        ;;
    aarch64-apple-darwin)
        source_file="$artifact_dir/libiron_vaak_native.dylib"
        unity_file="$unity_root/macOS/ARM64/libiron_vaak_native.dylib"
        nuget_file="$nuget_root/osx-arm64/native/libiron_vaak_native.dylib"
        ;;
    aarch64-linux-android)
        source_file="$artifact_dir/libiron_vaak_native.so"
        unity_file="$unity_root/Android/libs/arm64-v8a/libiron_vaak_native.so"
        nuget_file="$nuget_root/android-arm64/native/libiron_vaak_native.so"
        ;;
    x86_64-linux-android)
        source_file="$artifact_dir/libiron_vaak_native.so"
        unity_file="$unity_root/Android/libs/x86_64/libiron_vaak_native.so"
        nuget_file="$nuget_root/android-x64/native/libiron_vaak_native.so"
        ;;
    aarch64-apple-ios)
        source_file="$artifact_dir/libiron_vaak_native.a"
        unity_file="$unity_root/iOS/libiron_vaak_native.a"
        nuget_file="$nuget_root/ios-arm64/native/libiron_vaak_native.a"
        ;;
    *)
        echo "unsupported staging target: $target" >&2
        exit 2
        ;;
esac

install -D "$source_file" "$unity_file"
install -D "$source_file" "$nuget_file"
echo "staged $target"
