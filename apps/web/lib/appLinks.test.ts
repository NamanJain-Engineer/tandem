import { describe, expect, it } from "vitest";
import { appleAppSiteAssociation, assetLinks } from "./appLinks";

const FP = Array.from({ length: 32 }, () => "AB").join(":");

describe("F-RM-05 app links", () => {
  it("serves nothing until the owner's ids exist", () => {
    expect(appleAppSiteAssociation(undefined)).toBeNull();
    expect(appleAppSiteAssociation("not-a-team")).toBeNull();
    expect(assetLinks("")).toBeNull();
    expect(assetLinks("zz")).toBeNull();
  });

  it("points /remote links at Vibezeo Remote on both platforms", () => {
    expect(appleAppSiteAssociation("ABCDE12345")).toEqual({
      applinks: { details: [{ appIDs: ["ABCDE12345.com.vibezeo.remote"], components: [{ "/": "/remote*" }] }] },
    });
    expect(assetLinks(`${FP.toLowerCase()}, junk`)?.[0]?.target.sha256_cert_fingerprints).toEqual([FP]);
  });
});
