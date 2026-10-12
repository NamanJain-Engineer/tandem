#!/usr/bin/env bash
# Builds Vibezeo Remote for Android (F-RM-05): a signed release APK (direct
# download, GitHub release) and AAB (Google Play), locally, with no paid
# service. Needs the Android SDK and NDK and JDK 17+ (docs/OPERATING.md §10).
#
#   bash scripts/mobile/build-android.sh                       # the owner's upload key
#   bash scripts/mobile/build-android.sh --generate-dev-key    # a throwaway key (testing only)
#
# The key and its passwords are read from $VIBEZEO_ANDROID_KEYS (default
# ~/.vibezeo/android): upload.jks and upload.properties. They never go in the
# repository. Outputs land in dist/mobile/ with SHA256SUMS.
set -euo pipefail
cd "$(dirname "$0")/../.."
root="$(pwd)"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/android/sdk}"
export ANDROID_SDK_ROOT="$ANDROID_HOME"
keys="${VIBEZEO_ANDROID_KEYS:-$HOME/.vibezeo/android}"
props="$keys/upload.properties"
version="$(node -p "require('./apps/mobile/package.json').version")"
bt="$ANDROID_HOME/build-tools/36.0.0"

if [ ! -f "$props" ]; then
  if [ "${1:-}" != "--generate-dev-key" ]; then
    echo "No upload key in $keys. Make yours (HUMAN_TODO \"Vibezeo Remote: Android signing\") or pass --generate-dev-key for a throwaway one." >&2
    exit 1
  fi
  mkdir -p "$keys" && chmod 700 "$keys"
  VZ_PW="$(head -c 30 /dev/urandom | base64 | tr '+/' '-_' | tr -d '=\n')"
  export VZ_PW
  keytool -genkeypair -storetype PKCS12 -keystore "$keys/upload.jks" -alias upload \
    -keyalg RSA -keysize 4096 -validity 10000 -storepass:env VZ_PW -keypass:env VZ_PW \
    -dname "CN=Vibezeo Remote throwaway upload key, O=Development build" >/dev/null
  umask 077
  printf 'STORE_FILE=%s\nSTORE_PASSWORD=%s\nKEY_ALIAS=upload\nKEY_PASSWORD=%s\n' "$keys/upload.jks" "$VZ_PW" "$VZ_PW" > "$props"
  unset VZ_PW
  echo "Made a throwaway upload key in $keys (not for publishing: see HUMAN_TODO)."
fi

# Read the four values without executing the file.
get() { sed -n "s/^$1=//p" "$props" | head -1; }
export ORG_GRADLE_PROJECT_VIBEZEO_UPLOAD_STORE_FILE="$(get STORE_FILE)"
export ORG_GRADLE_PROJECT_VIBEZEO_UPLOAD_STORE_PASSWORD="$(get STORE_PASSWORD)"
export ORG_GRADLE_PROJECT_VIBEZEO_UPLOAD_KEY_ALIAS="$(get KEY_ALIAS)"
export ORG_GRADLE_PROJECT_VIBEZEO_UPLOAD_KEY_PASSWORD="$(get KEY_PASSWORD)"
test -f "$ORG_GRADLE_PROJECT_VIBEZEO_UPLOAD_STORE_FILE" || { echo "Keystore missing: $ORG_GRADLE_PROJECT_VIBEZEO_UPLOAD_STORE_FILE" >&2; exit 1; }

cd "$root/apps/mobile"
CI=1 EXPO_NO_TELEMETRY=1 npx expo prebuild --platform android --clean --no-install
cd android
./gradlew --no-daemon --console=plain \
  -PreactNativeArchitectures=arm64-v8a,armeabi-v7a,x86_64 \
  :app:assembleRelease :app:bundleRelease

out="$root/dist/mobile"
mkdir -p "$out"
apk="$out/VibezeoRemote-$version.apk"
aab="$out/VibezeoRemote-$version.aab"
cp app/build/outputs/apk/release/app-release.apk "$apk"
cp app/build/outputs/bundle/release/app-release.aab "$aab"

# Signed with the upload key, not the debug key; 16 KB page alignment (Play).
certs="$("$bt/apksigner" verify --print-certs "$apk")"
echo "$certs" | grep -q "Android Debug" && { echo "The APK is signed with the debug key" >&2; exit 1; }
echo "$certs" | grep "Signer #1 certificate DN"
"$bt/zipalign" -c -P 16 4 "$apk" && echo "zipalign: 16 KB aligned"
jarsigner -verify "$aab" >/dev/null && echo "aab: signed"
(cd "$out" && sha256sum "$(basename "$apk")" "$(basename "$aab")" | tee SHA256SUMS)
