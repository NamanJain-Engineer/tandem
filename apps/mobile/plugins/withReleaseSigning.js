// Signs Android release builds with the owner's upload key when the build
// passes one (Gradle properties VIBEZEO_UPLOAD_*, set by
// scripts/mobile/build-android.sh from a file outside the repository).
// Without them the release build keeps Expo's default (the debug key), and
// the build script refuses to publish such an artifact. Keys and passwords
// never enter the repository (HUMAN_TODO "Vibezeo Remote: Android signing").
const { withAppBuildGradle } = require("expo/config-plugins");

const RELEASE = `        release {
            if (findProperty('VIBEZEO_UPLOAD_STORE_FILE')) {
                storeFile file(findProperty('VIBEZEO_UPLOAD_STORE_FILE'))
                storePassword findProperty('VIBEZEO_UPLOAD_STORE_PASSWORD')
                keyAlias findProperty('VIBEZEO_UPLOAD_KEY_ALIAS')
                keyPassword findProperty('VIBEZEO_UPLOAD_KEY_PASSWORD')
            }
        }
`;

function addReleaseSigning(gradle) {
  if (gradle.includes("VIBEZEO_UPLOAD_STORE_FILE")) return gradle;
  const configs = /signingConfigs\s*\{\s*\n/;
  if (!configs.test(gradle)) throw new Error("withReleaseSigning: no signingConfigs block in app/build.gradle");
  let out = gradle.replace(configs, (m) => m + RELEASE);
  const release = /(buildTypes\s*\{[\s\S]*?release\s*\{[\s\S]*?)signingConfig signingConfigs\.debug/;
  if (!release.test(out)) throw new Error("withReleaseSigning: no release signingConfig in app/build.gradle");
  out = out.replace(
    release,
    "$1signingConfig(findProperty('VIBEZEO_UPLOAD_STORE_FILE') ? signingConfigs.release : signingConfigs.debug)",
  );
  return out;
}

module.exports = function withReleaseSigning(config) {
  return withAppBuildGradle(config, (c) => {
    c.modResults.contents = addReleaseSigning(c.modResults.contents);
    return c;
  });
};
module.exports.addReleaseSigning = addReleaseSigning;
