import { describe, expect, it } from "vitest";
import { mailMention } from "./mention";

const TEAM = [
  { id: "a", name: "Ada" },
  { id: "b", name: "Bea" },
  { id: "bb", name: "Bea Two" },
];

describe("F-AG-12 @name in a teammate composer", () => {
  it("F-AG-12 @name at the start is mail to that teammate, with the rest as the body", () => {
    expect(mailMention("@bea check CI on main", TEAM, "a")).toEqual({
      to: TEAM[1],
      body: "check CI on main",
    });
    expect(mailMention("  @Bea Two\nplease look", TEAM, "a")).toEqual({ to: TEAM[2], body: "please look" });
  });

  it("F-AG-12 anything else is an ordinary message", () => {
    expect(mailMention("hi @Bea", TEAM, "a")).toBeNull();
    expect(mailMention("@Beatrice hi", TEAM, "a")).toBeNull();
    expect(mailMention("@Nobody hi", TEAM, "a")).toBeNull();
    // A teammate doesn't mail itself.
    expect(mailMention("@Ada hi", TEAM, "a")).toBeNull();
  });
});
