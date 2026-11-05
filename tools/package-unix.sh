#!/bin/bash
# Builds dist/pob-rust-<version>-<linux|macos>-<x64|arm64>.tar.gz: the exe of this system plus the files of a Windows package, which do not depend on the system.
#   tools/package-unix.sh <folder of an unpacked release zip, or dist/pob-rust>
# Nothing is published or downloaded except the crates cargo needs.
set -euo pipefail

src=${1:?usage: tools/package-unix.sh <folder of an unpacked release package>}
repo=$(cd "$(dirname "$0")/.." && pwd)
[ -f "$src/data.lock" ] && [ -f "$src/src/src.cache" ] || { echo "$src is not a package: no data.lock or src/src.cache" >&2; exit 1; }
version=$(sed -nE 's/^version *= *"([^"]+)".*/\1/p' "$repo/Cargo.toml" | head -1)
case "$(uname -s)" in Linux) os=linux ;; Darwin) os=macos ;; *) echo "unknown system" >&2; exit 1 ;; esac
case "$(uname -m)" in x86_64) arch=x64 ;; arm64 | aarch64) arch=arm64 ;; *) echo "unknown machine" >&2; exit 1 ;; esac

# No path of this machine in the exe: the sources of the dependencies and of the workspace appear in panic locations
cargo_home=${CARGO_HOME:-$HOME/.cargo}
rustup_home=${RUSTUP_HOME:-$HOME/.rustup}
cd "$repo"
export RUSTFLAGS="--remap-path-prefix=$repo=src --remap-path-prefix=$cargo_home=cargo --remap-path-prefix=$rustup_home=rustup"
if [ "$os" = linux ]; then
    # A plain build asks for the glibc of the machine it was built on: this one runs from glibc 2.28 on. Needs zig and cargo-zigbuild.
    target=$(uname -m)-unknown-linux-gnu
    cargo zigbuild --profile dist -p pob-app --features package --target "$target.2.28"
    exe=target/$target/dist/pob-app
else
    cargo build --profile dist -p pob-app --features package
    exe=target/dist/pob-app
fi
for bad in "$HOME" "$repo" "$(whoami)"; do
    if LC_ALL=C grep -aqF -- "$bad" "$exe"; then echo "the exe contains: $bad" >&2; exit 1; fi
done

name=pob-rust-$version-$os-$arch
out=dist/$name
rm -rf "$out"
mkdir -p "$out"
(cd "$src" && tar --exclude='*.exe' -cf - .) | tar -C "$out" -xf -
cp "$exe" "$out/Path of Building (Rust)"
tar -C dist -czf "dist/$name.tar.gz" "$name"
rm -rf "$out"
ls -l "dist/$name.tar.gz"
if command -v sha256sum >/dev/null; then sha256sum "dist/$name.tar.gz"; else shasum -a 256 "dist/$name.tar.gz"; fi
