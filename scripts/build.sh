#!/usr/bin/env bash
# Build the recovery for one device, as the workflow builds it: the tools,
# the stock recovery taken out of the phone's package, and the image.
set -euo pipefail

cd "$(dirname "$0")/.."

device="${1:-}"
if [ -z "$device" ]; then
    echo "say which device to build for, as in: ./scripts/build.sh piano" >&2
    exit 1
fi

export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-$HOME/android-ndk-r29}"

cargo ndk -t arm64-v8a --platform 30 build --locked --release -p skyward
RUSTFLAGS="-C target-feature=+crt-static" \
    cargo ndk -t arm64-v8a --platform 30 build --locked --release -p skyward-system-init --target-dir target/system-init
cargo build --locked --release -p skyward-pack

rm -rf tools stock out
mkdir -p tools stock out
cp target/aarch64-linux-android/release/skyward tools/
cp target/system-init/aarch64-linux-android/release/system-init tools/system-init
cp target/release/skyward-pack tools/

chmod +x tools/skyward tools/skyward-pack

version="$(git rev-parse --short=7 HEAD)"
# What the stock build is called, read before the partition is taken out: the
# read alone asks the package's server for its directory.
stock="$(tools/skyward-pack stock --config "devices/$device.toml" | sed -n 's/^version=//p')"
[ -n "$stock" ] || { echo "the packer named no stock version for $device" >&2; exit 1; }
tools/skyward-pack stock --config "devices/$device.toml" --out stock/recovery.img

tools/skyward-pack recovery --stock stock/recovery.img --binary tools/skyward --system-init tools/system-init \
    --config "devices/$device.toml" --font resources/fonts/MiSansVF.ttf --mono resources/fonts/KirkMono-Regular.ttf \
    --mono-license resources/fonts/KirkMono-OFL.txt --out "out/skyward-$version-$device-$stock.img"

ls -l out
