#!/usr/bin/env bash
set -euo pipefail

PLUGIN_NAME="nebula_cluster"
PLUGIN_DISPLAY="Nebula Cluster"
PLUGIN_VERSION="1.0.0"
BUNDLE_ID="audio.nebula.cluster"
AUV2_TYPE="aufx"
AUV2_SUBTYPE="NClu"
AUV2_MANUFACTURER="NbAu"
AUV2_VERSION_INT="65536"

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "[ERROR] This script is for macOS only."
    exit 1
fi

if ! xcode-select -p >/dev/null 2>&1; then
    echo "[ERROR] Xcode Command Line Tools not found."
    echo "Install with: xcode-select --install"
    exit 1
fi

if ! command -v rustc >/dev/null 2>&1; then
    echo "[ERROR] Rust compiler not found."
    echo "Install Rust from: https://www.rust-lang.org/tools/install"
    exit 1
fi

echo "== Nebula Cluster macOS Universal build =="
echo "[*] Adding Rust targets"
rustup target add aarch64-apple-darwin x86_64-apple-darwin

export CARGO_INCREMENTAL=0
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-11.0}"

echo "[*] Building Apple Silicon"
cargo build --release --target aarch64-apple-darwin

echo "[*] Building Intel"
cargo build --release --target x86_64-apple-darwin

echo "[*] Creating CLAP and VST3 universal bundles"
cargo xtask bundle-universal nebula_cluster --release

PLUGIN_LIB="lib${PLUGIN_NAME}.dylib"
AARCH64_LIB="target/aarch64-apple-darwin/release/${PLUGIN_LIB}"
X86_64_LIB="target/x86_64-apple-darwin/release/${PLUGIN_LIB}"
UNIVERSAL_DIR="target/universal"
UNIVERSAL_LIB="${UNIVERSAL_DIR}/${PLUGIN_LIB}"

mkdir -p "${UNIVERSAL_DIR}"
lipo -create "${AARCH64_LIB}" "${X86_64_LIB}" -output "${UNIVERSAL_LIB}"
lipo -info "${UNIVERSAL_LIB}"

echo "[*] Creating AUv2 component"
AUV2_BUNDLE="target/bundled/${PLUGIN_DISPLAY}.component"
rm -rf "${AUV2_BUNDLE}"
mkdir -p "${AUV2_BUNDLE}/Contents/MacOS"
cp "${UNIVERSAL_LIB}" "${AUV2_BUNDLE}/Contents/MacOS/${PLUGIN_NAME}"

cat > "${AUV2_BUNDLE}/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>AudioComponents</key>
    <array>
        <dict>
            <key>description</key>
            <string>${PLUGIN_DISPLAY}</string>
            <key>factoryFunction</key>
            <string>GetPluginFactoryAUV2</string>
            <key>manufacturer</key>
            <string>${AUV2_MANUFACTURER}</string>
            <key>name</key>
            <string>Nebula Audio: ${PLUGIN_DISPLAY}</string>
            <key>subtype</key>
            <string>${AUV2_SUBTYPE}</string>
            <key>type</key>
            <string>${AUV2_TYPE}</string>
            <key>version</key>
            <integer>${AUV2_VERSION_INT}</integer>
        </dict>
    </array>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>${PLUGIN_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}.auv2</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>${PLUGIN_DISPLAY}</string>
    <key>CFBundlePackageType</key>
    <string>BNDL</string>
    <key>CFBundleShortVersionString</key>
    <string>${PLUGIN_VERSION}</string>
    <key>CFBundleSignature</key>
    <string>${AUV2_MANUFACTURER}</string>
    <key>CFBundleVersion</key>
    <string>${PLUGIN_VERSION}</string>
    <key>CSResourcesFileMapped</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>${MACOSX_DEPLOYMENT_TARGET}</string>
    <key>NSHumanReadableCopyright</key>
    <string>Copyright (c) Nebula Audio. All rights reserved.</string>
</dict>
</plist>
PLIST

echo "[OK] CLAP/VST3 bundles: target/bundled/"
echo "[OK] AUv2 component: ${AUV2_BUNDLE}"
