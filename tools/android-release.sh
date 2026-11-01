#!/bin/bash
# Signed release APK. The key and its details stay in the gitignored .private
# folder: sweaxdrop-release.jks and keystore.txt ("alias: ..." and "password: ...").
#   tools/android-release.sh   ->  apps/android/app/build/outputs/apk/release/app-release.apk
set -e
R="$(cd "$(dirname "$0")/.." && pwd)"
K="$R/.private/keystore.txt"
field() { grep -i "^$1" "$K" | sed 's/^[^:=]*[:=] *//' | tr -d '\r'; }
export JAVA_HOME="${JAVA_HOME:-/c/Program Files/Android/Android Studio/jbr}"
export SWEAXDROP_KEYSTORE="$R/.private/sweaxdrop-release.jks"
export SWEAXDROP_KEY_ALIAS="$(field alias)"
export SWEAXDROP_KEYSTORE_PASSWORD="$(field password)"
cd "$R/apps/android"
./gradlew assembleRelease --no-daemon -q
ls -la app/build/outputs/apk/release/app-release.apk
