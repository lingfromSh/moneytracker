#!/usr/bin/env bash
# Builds a signed release APK.
#
# Prerequisites (one-time):
#   sdkmanager "platform-tools" "platforms;android-35" "build-tools;35.0.0" "ndk;27.2.12479018"
#   rustup target add aarch64-linux-android
#   a keystore, created by this script on first run
#
# Usage: scripts/android-apk.sh [arm64|armv7|x86_64]
set -euo pipefail

cd "$(dirname "$0")/.."

TARGET=${1:-arm64}
ARCH="aarch64"
[ "$TARGET" = "armv7" ] && ARCH="armv7"
[ "$TARGET" = "x86_64" ] && ARCH="x86_64"

ANDROID_HOME=${ANDROID_HOME:-$HOME/Library/Android/sdk}
NDK_HOME=${NDK_HOME:-$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | tail -1)}
BUILD_TOOLS=$(ls -d "$ANDROID_HOME"/build-tools/* 2>/dev/null | tail -1)
KEYSTORE=${KEYSTORE:-$HOME/.moneytracker-android/moneytracker.jks}
# 口令只存在本机：环境变量优先，其次是 pass 文件。仓库里不留任何默认口令。
PASS_FILE=${PASS_FILE:-$HOME/.moneytracker-android/pass}
if [ -z "${KEYSTORE_PASS:-}" ] && [ -f "$PASS_FILE" ]; then
  KEYSTORE_PASS=$(cat "$PASS_FILE")
fi
if [ -z "${KEYSTORE_PASS:-}" ]; then
  echo "缺少签名口令：export KEYSTORE_PASS=… 或把它写进 ${PASS_FILE}（chmod 600，不要提交）" >&2
  exit 1
fi
KEY_ALIAS=${KEY_ALIAS:-moneytracker}

if [ ! -x "$BUILD_TOOLS/apksigner" ]; then
  echo "没有找到 Android build-tools，请先装 platform-tools / platforms;android-35 / build-tools;35.0.0" >&2
  exit 1
fi
if [ ! -f "$KEYSTORE" ]; then
  echo "==> 生成签名密钥 ${KEYSTORE}（请备份，升级必须用同一把）"
  mkdir -p "$(dirname "$KEYSTORE")"
  keytool -genkeypair -v \
    -keystore "$KEYSTORE" -alias "$KEY_ALIAS" -keyalg RSA -keysize 2048 -validity 10000 \
    -storepass "$KEYSTORE_PASS" -keypass "$KEYSTORE_PASS" \
    -dname "CN=MoneyTracker, OU=Personal, O=Personal, L=Shanghai, ST=Shanghai, C=CN" >/dev/null
  umask 077
  printf '%s' "$KEYSTORE_PASS" > "$PASS_FILE"
  echo "==> 口令已写入 ${PASS_FILE}（只在本机，别提交）"
fi

echo "==> 编译（release / ${ARCH}）"
ANDROID_HOME=$ANDROID_HOME NDK_HOME=$NDK_HOME pnpm tauri android build --apk --target "$ARCH"

UNSIGNED="src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release-unsigned.apk"
if [ ! -f "$UNSIGNED" ]; then
  echo "没有找到未签名 APK：$UNSIGNED" >&2
  exit 1
fi

VERSION=$(node -p "require('./src-tauri/tauri.conf.json').version")
OUT="build/android/moneytracker-$VERSION-$TARGET.apk"
mkdir -p build/android
echo "==> 签名"
"$BUILD_TOOLS/zipalign" -p -f 4 "$UNSIGNED" build/android/aligned.apk
"$BUILD_TOOLS/apksigner" sign \
  --ks "$KEYSTORE" --ks-pass "pass:$KEYSTORE_PASS" --key-pass "pass:$KEYSTORE_PASS" \
  --ks-key-alias "$KEY_ALIAS" --out "$OUT" build/android/aligned.apk
"$BUILD_TOOLS/apksigner" verify --verbose "$OUT" | head -3
rm -f build/android/aligned.apk

echo "==> $OUT ($(du -h "$OUT" | cut -f1))"
echo "    adb install -r \"$OUT\""
