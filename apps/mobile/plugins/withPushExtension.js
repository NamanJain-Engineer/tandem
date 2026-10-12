// Adds the iOS Notification Service Extension that opens sealed push alerts
// (F-RM-06, D-2022): its files, an app_extension target embedded in the app,
// and the shared keychain group both use (so the extension can read the
// alert key the app stores). Signing and the APNs entitlement come from EAS.
const fs = require("fs");
const path = require("path");
const { withDangerousMod, withEntitlementsPlist, withXcodeProject } = require("expo/config-plugins");

const NAME = "VibezeoPushExtension";
const SOURCES = ["NotificationService.swift"];
const FILES = [...SOURCES, "Info.plist", "PrivacyInfo.xcprivacy"];
const GROUP = "$(AppIdentifierPrefix)com.vibezeo.remote.shared";

function entitlements() {
  return `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>keychain-access-groups</key>
  <array>
    <string>${GROUP}</string>
  </array>
</dict>
</plist>
`;
}

function withFiles(config) {
  return withDangerousMod(config, [
    "ios",
    async (c) => {
      const dir = path.join(c.modRequest.platformProjectRoot, NAME);
      fs.mkdirSync(dir, { recursive: true });
      for (const f of FILES) fs.copyFileSync(path.join(__dirname, "ios-push-extension", f), path.join(dir, f));
      fs.writeFileSync(path.join(dir, `${NAME}.entitlements`), entitlements());
      return c;
    },
  ]);
}

function hasTarget(project) {
  const targets = project.pbxNativeTargetSection();
  return Object.values(targets).some((t) => typeof t === "object" && (t.name === NAME || t.name === `"${NAME}"`));
}

/** Adds the target to the Xcode project (once). Exported for tests. */
function addExtensionTarget(project, { bundleId, deploymentTarget, teamId }) {
  if (hasTarget(project)) return project;
  const target = project.addTarget(NAME, "app_extension", NAME, bundleId);
  const group = project.addPbxGroup([...FILES, `${NAME}.entitlements`], NAME, NAME);
  const groups = project.hash.project.objects.PBXGroup;
  for (const key of Object.keys(groups)) {
    const g = groups[key];
    if (typeof g === "object" && g.name === undefined && g.path === undefined && Array.isArray(g.children)) {
      g.children.push({ value: group.uuid, comment: NAME });
      break;
    }
  }
  project.addBuildPhase(SOURCES, "PBXSourcesBuildPhase", "Sources", target.uuid);
  project.addBuildPhase(["PrivacyInfo.xcprivacy"], "PBXResourcesBuildPhase", "Resources", target.uuid);
  project.addBuildPhase([], "PBXFrameworksBuildPhase", "Frameworks", target.uuid);
  const configs = project.pbxXCBuildConfigurationSection();
  for (const key of Object.keys(configs)) {
    const s = configs[key].buildSettings;
    if (!s || (s.PRODUCT_NAME !== `"${NAME}"` && s.PRODUCT_NAME !== NAME)) continue;
    Object.assign(s, {
      INFOPLIST_FILE: `${NAME}/Info.plist`,
      CODE_SIGN_ENTITLEMENTS: `${NAME}/${NAME}.entitlements`,
      CODE_SIGN_STYLE: "Automatic",
      SWIFT_VERSION: "5.0",
      IPHONEOS_DEPLOYMENT_TARGET: deploymentTarget,
      TARGETED_DEVICE_FAMILY: '"1,2"',
      PRODUCT_BUNDLE_IDENTIFIER: bundleId,
      MARKETING_VERSION: "1.0",
      CURRENT_PROJECT_VERSION: "1",
      GENERATE_INFOPLIST_FILE: "NO",
    });
    if (teamId) s.DEVELOPMENT_TEAM = teamId;
  }
  return project;
}

function withPushExtension(config) {
  config = withFiles(config);
  config = withEntitlementsPlist(config, (c) => {
    const groups = new Set(c.modResults["keychain-access-groups"] ?? []);
    groups.add(GROUP);
    c.modResults["keychain-access-groups"] = [...groups];
    return c;
  });
  return withXcodeProject(config, (c) => {
    addExtensionTarget(c.modResults, {
      bundleId: `${c.ios?.bundleIdentifier ?? "com.vibezeo.remote"}.push`,
      deploymentTarget: "16.4",
      teamId: process.env.APPLE_TEAM_ID,
    });
    return c;
  });
}

module.exports = withPushExtension;
module.exports.addExtensionTarget = addExtensionTarget;
module.exports.EXTENSION_NAME = NAME;
module.exports.hasTarget = hasTarget;
