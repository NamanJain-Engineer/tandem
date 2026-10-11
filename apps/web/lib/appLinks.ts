// Universal Links and Android App Links for Vibezeo Remote (F-RM-05, D-2026):
// `https://app.vibezeo.com/remote#p=…` opens the app when it's installed.
// They need the owner's Apple team id and the Play app-signing certificate
// (HUMAN_TODO); until they're set, both files answer 404.
export const APP_ID = "com.vibezeo.remote";

export function appleAppSiteAssociation(teamId: string | undefined) {
  if (!teamId || !/^[A-Z0-9]{10}$/.test(teamId)) return null;
  return {
    applinks: { details: [{ appIDs: [`${teamId}.${APP_ID}`], components: [{ "/": "/remote*" }] }] },
  };
}

export function assetLinks(fingerprints: string | undefined) {
  const prints = (fingerprints ?? "")
    .split(",")
    .map((s) => s.trim().toUpperCase())
    .filter((s) => /^([0-9A-F]{2}:){31}[0-9A-F]{2}$/.test(s));
  if (prints.length === 0) return null;
  return [
    {
      relation: ["delegate_permission/common.handle_all_urls"],
      target: { namespace: "android_app", package_name: APP_ID, sha256_cert_fingerprints: prints },
    },
  ];
}
