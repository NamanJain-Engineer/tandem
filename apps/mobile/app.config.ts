// Vibezeo Remote (F-RM-05, D-2000): the phone companion as a native app,
// built with Expo. Android builds locally (`scripts/mobile/build-android.sh`);
// iOS builds on EAS once the owner has Expo and Apple accounts (HUMAN_TODO).
import type { ExpoConfig } from "expo/config";

const teamId = process.env.APPLE_TEAM_ID;
/** The keychain group the app and its push extension share (D-2022). */
const keychainGroup = teamId ? `${teamId}.com.vibezeo.remote.shared` : undefined;

const config: ExpoConfig = {
  name: "Vibezeo Remote",
  slug: "vibezeo-remote",
  owner: process.env.EXPO_OWNER,
  version: "0.1.0",
  scheme: "vibezeo-remote",
  orientation: "portrait",
  userInterfaceStyle: "dark",
  backgroundColor: "#09090b",
  icon: "./assets/icon.png",
  ios: {
    bundleIdentifier: "com.vibezeo.remote",
    buildNumber: "1",
    supportsTablet: true,
    // Universal links: app.vibezeo.com serves apple-app-site-association (apps/web).
    associatedDomains: ["applinks:app.vibezeo.com"],
    // The app collects nothing and doesn't track (D-2024). These are the
    // required-reason APIs the Expo and React Native modules use.
    privacyManifests: {
      NSPrivacyTracking: false,
      NSPrivacyTrackingDomains: [],
      NSPrivacyCollectedDataTypes: [],
      NSPrivacyAccessedAPITypes: [
        { NSPrivacyAccessedAPIType: "NSPrivacyAccessedAPICategoryUserDefaults", NSPrivacyAccessedAPITypeReasons: ["CA92.1"] },
        { NSPrivacyAccessedAPIType: "NSPrivacyAccessedAPICategoryFileTimestamp", NSPrivacyAccessedAPITypeReasons: ["C617.1"] },
        { NSPrivacyAccessedAPIType: "NSPrivacyAccessedAPICategorySystemBootTime", NSPrivacyAccessedAPITypeReasons: ["35F9.1"] },
        { NSPrivacyAccessedAPIType: "NSPrivacyAccessedAPICategoryDiskSpace", NSPrivacyAccessedAPITypeReasons: ["E174.1"] },
      ],
    },
    infoPlist: {
      // The app's own end-to-end encryption (X25519, XChaCha20-Poly1305,
      // ChaCha20-Poly1305) is standard, non-proprietary encryption; Apple
      // counts it as non-exempt. D-2025 and HUMAN_TODO: the export compliance
      // answer and BIS self-classification report.
      ITSAppUsesNonExemptEncryption: true,
      NSCameraUsageDescription: "The camera scans the pairing code your computer shows.",
      NSLocalNetworkUsageDescription: "Vibezeo Remote connects to your computer on your local network.",
      // LAN mode (F-RM-04): the computer's relay is plain ws:// on a private
      // address. Frames are end-to-end encrypted, and the app refuses ws://
      // anywhere but private and VPN addresses (relayAllowed).
      NSAppTransportSecurity: { NSAllowsLocalNetworking: true },
    },
  },
  android: {
    package: "com.vibezeo.remote",
    versionCode: 1,
    adaptiveIcon: {
      foregroundImage: "./assets/adaptive-icon.png",
      monochromeImage: "./assets/adaptive-monochrome.png",
      backgroundColor: "#0b1220",
    },
    permissions: ["android.permission.CAMERA", "android.permission.POST_NOTIFICATIONS"],
    // Only what the app uses: camera, notifications, network. Libraries ask
    // for more; these are removed from the merged manifest.
    blockedPermissions: [
      "android.permission.RECORD_AUDIO",
      "android.permission.SYSTEM_ALERT_WINDOW",
      "android.permission.READ_EXTERNAL_STORAGE",
      "android.permission.WRITE_EXTERNAL_STORAGE",
      "android.permission.USE_BIOMETRIC",
      "android.permission.USE_FINGERPRINT",
      "com.google.android.finsky.permission.BIND_GET_INSTALL_REFERRER_SERVICE",
      "com.sec.android.provider.badge.permission.READ",
      "com.sec.android.provider.badge.permission.WRITE",
      "com.htc.launcher.permission.READ_SETTINGS",
      "com.htc.launcher.permission.UPDATE_SHORTCUT",
      "com.sonyericsson.home.permission.BROADCAST_BADGE",
      "com.sonymobile.home.permission.PROVIDER_INSERT_BADGE",
      "com.anddoes.launcher.permission.UPDATE_COUNT",
      "com.majeur.launcher.permission.UPDATE_BADGE",
      "com.huawei.android.launcher.permission.CHANGE_BADGE",
      "com.huawei.android.launcher.permission.READ_SETTINGS",
      "com.huawei.android.launcher.permission.WRITE_SETTINGS",
      "com.oppo.launcher.permission.READ_SETTINGS",
      "com.oppo.launcher.permission.WRITE_SETTINGS",
      "me.everything.badger.permission.BADGE_COUNT_READ",
      "me.everything.badger.permission.BADGE_COUNT_WRITE",
    ],
    // FCM (F-RM-06) needs the owner's Firebase file; without it the app has no push (HUMAN_TODO).
    googleServicesFile: process.env.GOOGLE_SERVICES_JSON,
    intentFilters: [
      {
        action: "VIEW",
        data: [{ scheme: "vibezeo-remote", host: "pair" }],
        category: ["BROWSABLE", "DEFAULT"],
      },
      // App Links: app.vibezeo.com serves assetlinks.json (apps/web).
      {
        action: "VIEW",
        autoVerify: true,
        data: [{ scheme: "https", host: "app.vibezeo.com", pathPrefix: "/remote" }],
        category: ["BROWSABLE", "DEFAULT"],
      },
    ],
  },
  plugins: [
    "./plugins/withReleaseSigning",
    "./plugins/withPushExtension",
    "expo-secure-store",
    ["expo-splash-screen", { image: "./assets/splash-icon.png", imageWidth: 160, backgroundColor: "#09090b" }],
    [
      "expo-camera",
      {
        cameraPermission: "The camera scans the pairing code your computer shows.",
        recordAudioAndroid: false,
        barcodeScannerEnabled: true,
      },
    ],
    ["expo-notifications", { icon: "./assets/notification-icon.png", color: "#2DD4BF", mode: "production" }],
    [
      "expo-build-properties",
      {
        android: {
          compileSdkVersion: 36,
          targetSdkVersion: 36,
          minSdkVersion: 24,
          usesCleartextTraffic: true,
        },
        ios: { deploymentTarget: "16.4" },
      },
    ],
  ],
  extra: {
    // Push needs the owner's credentials in the build (HUMAN_TODO): Firebase
    // on Android, the Apple team (for the shared keychain) on iOS.
    pushAndroid: !!process.env.GOOGLE_SERVICES_JSON,
    pushIos: process.env.APNS_ENABLED === "1",
    keychainGroup,
    eas: {
      ...(process.env.EAS_PROJECT_ID ? { projectId: process.env.EAS_PROJECT_ID } : {}),
      build: {
        experimental: {
          ios: {
            appExtensions: [
              {
                targetName: "VibezeoPushExtension",
                bundleIdentifier: "com.vibezeo.remote.push",
                entitlements: { "keychain-access-groups": ["$(AppIdentifierPrefix)com.vibezeo.remote.shared"] },
              },
            ],
          },
        },
      },
    },
  },
};

export default config;
