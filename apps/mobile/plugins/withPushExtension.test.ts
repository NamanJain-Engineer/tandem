import { createRequire } from "node:module";
import { describe, expect, it } from "vitest";

const require = createRequire(import.meta.url);
const xcode = require("xcode") as { project: (p: string) => { parseSync(): XcodeProject } };
type XcodeProject = {
  pbxTargetByName(n: string): unknown;
  pbxNativeTargetSection(): Record<string, unknown>;
  writeSync(): string;
  addTarget(...a: unknown[]): { uuid: string };
  addPbxGroup(...a: unknown[]): { uuid: string };
  addBuildPhase(...a: unknown[]): unknown;
  pbxXCBuildConfigurationSection(): Record<string, { buildSettings?: Record<string, string> }>;
  hash: { project: { objects: { PBXGroup: Record<string, unknown> } } };
};
const { addExtensionTarget, hasTarget, EXTENSION_NAME } = require("./withPushExtension.js") as {
  hasTarget: (p: XcodeProject) => boolean;
  addExtensionTarget: (p: XcodeProject, o: { bundleId: string; deploymentTarget: string; teamId?: string }) => XcodeProject;
  EXTENSION_NAME: string;
};

describe("F-RM-06 the iOS push extension", () => {
  it("F-RM-06.2 adds an embedded app extension target with its entitlements, once", () => {
    const path = new URL("./__fixtures__/template.pbxproj", import.meta.url).pathname;
    const project = xcode.project(path).parseSync();
    addExtensionTarget(project, { bundleId: "com.vibezeo.remote.push", deploymentTarget: "16.4", teamId: "TEAM123" });
    addExtensionTarget(project, { bundleId: "com.vibezeo.remote.push", deploymentTarget: "16.4" });
    const text = project.writeSync();
    expect(hasTarget(project)).toBe(true);
    expect(text.match(/productType = "com\.apple\.product-type\.app-extension"/g)?.length).toBe(1);
    expect(text).toContain("NotificationService.swift in Sources");
    expect(text).toContain(`CODE_SIGN_ENTITLEMENTS = ${EXTENSION_NAME}/${EXTENSION_NAME}.entitlements`);
    expect(text).toContain("PRODUCT_BUNDLE_IDENTIFIER = com.vibezeo.remote.push");
    expect(text).toContain("DEVELOPMENT_TEAM = TEAM123");
    expect(text).toMatch(/Copy Files|Embed App Extensions/);
  });
});
