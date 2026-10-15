import { describe, expect, it } from "vitest";
import { siteButton } from "./SiteButton";

// The public site's calls to action (D-1835): the app's 6 px controls with
// depth, as classes for plain <a> elements (marketing pages run no script).
describe("siteButton", () => {
  it("F-WB-01 site buttons carry their variant and size, and the default is the secondary", () => {
    expect(siteButton("primary", "lg").split(" ")).toEqual(expect.arrayContaining(["btn", "btn-primary", "btn-lg"]));
    expect(siteButton().split(" ")).toEqual(expect.arrayContaining(["btn", "btn-secondary", "btn-md"]));
    expect(siteButton("ghost", "sm")).toContain("btn-ghost");
  });

  it("F-WB-01 site buttons show the focus ring and never lose it to an outline reset", () => {
    expect(siteButton("primary")).toContain("focus-visible:outline-none");
  });
});
