import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const { addReleaseSigning } = require("./withReleaseSigning.js") as { addReleaseSigning: (g: string) => string };

// The shape of the app/build.gradle that `expo prebuild` writes.
const GRADLE = `android {
    signingConfigs {
        debug {
            storeFile file('debug.keystore')
            storePassword 'android'
            keyAlias 'androiddebugkey'
            keyPassword 'android'
        }
    }
    buildTypes {
        debug {
            signingConfig signingConfigs.debug
        }
        release {
            signingConfig signingConfigs.debug
            minifyEnabled enableMinifyInReleaseBuilds
        }
    }
}`;

describe("F-RM-05.8 Android release signing", () => {
  it("signs release builds with the upload key when one is passed, and never stores it", () => {
    const out = addReleaseSigning(GRADLE);
    expect(out).toContain("storeFile file(findProperty('VIBEZEO_UPLOAD_STORE_FILE'))");
    expect(out).toContain("signingConfig(findProperty('VIBEZEO_UPLOAD_STORE_FILE') ? signingConfigs.release : signingConfigs.debug)");
    // Debug builds keep the debug key.
    expect(out).toMatch(/debug \{\n\s+signingConfig signingConfigs\.debug/);
    // No secret is written into the file, and running it twice changes nothing.
    expect(out).not.toMatch(/storePassword '(?!android)/);
    expect(addReleaseSigning(out)).toBe(out);
  });

  it("fails loudly when the template changes shape", () => {
    expect(() => addReleaseSigning("android {}")).toThrow(/signingConfigs/);
  });
});
