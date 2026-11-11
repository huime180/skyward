#!/usr/bin/env bash
# What the workflow's setup action does, done once in a codespace: the
# toolchain the repository names, the Android NDK the recovery is built
# with, and cargo-ndk.
set -euo pipefail

cd "$(dirname "$0")/.."

rustup toolchain install
rustup component add clippy

ndk="${ANDROID_NDK_HOME:-$HOME/android-ndk-r29}"
if [ ! -d "$ndk" ]; then
    echo "fetching the Android NDK r29 into $ndk"
    # Google's own address is not reached everywhere: the mirror carries the
    # same archive.
    for url in \
        https://dl.google.com/android/repository/android-ndk-r29-linux.zip \
        https://googledownloads.cn/android/repository/android-ndk-r29-linux.zip \
        https://mirrors.cloud.tencent.com/AndroidSDK/android-ndk-r29-linux.zip; do
        if curl -fSL --retry 2 -o /tmp/ndk.zip "$url"; then break; fi
    done
    unzip -q /tmp/ndk.zip -d "$HOME"
    rm -f /tmp/ndk.zip
fi
export ANDROID_NDK_HOME="$ndk"
echo "ANDROID_NDK_HOME=$ndk"

if ! command -v cargo-ndk >/dev/null; then
    cargo install cargo-ndk --version 4.1.2 --locked
fi

echo "ready: ./scripts/build.sh <codename>"
