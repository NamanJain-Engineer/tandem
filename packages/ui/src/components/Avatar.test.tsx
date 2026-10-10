import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AVATAR_HUES, Avatar, avatarHue, fnv1a32, hueIndex, initials } from "./Avatar";

describe("Avatar", () => {
  it("F-AG-01.2 the avatar for an id renders identically every time (snapshot)", () => {
    const id = "tm_01HZX4Q8VJ8Q0W5N2Y3B6C7D8E";
    const first = render(<Avatar name="Release Lead" id={id} size={48} />).container.innerHTML;
    const again = render(<Avatar name="Release Lead" id={id} size={48} />).container.innerHTML;
    expect(again).toBe(first);
    expect(first).toMatchSnapshot();
  });

  it("F-AG-01.2 hues come from a 12-hue palette hashed from the id, or the user's pick", () => {
    expect(AVATAR_HUES).toHaveLength(12);
    for (const h of AVATAR_HUES) expect(h > 15 && h < 345).toBe(true);
    // The same hash as tandem_core::teammates::fnv1a32.
    expect(fnv1a32("a")).toBe(0xe40c292c);
    expect(hueIndex("tm_x")).toBe(fnv1a32("tm_x") % 12);
    expect(hueIndex("tm_x", 3)).toBe(3);
    expect(hueIndex("tm_x", 12)).toBe(fnv1a32("tm_x") % 12);
    expect(avatarHue("tm_x", 0)).toBe(25);
    const picked = render(<Avatar name="Scout" id="tm_x" hue={0} />).container.innerHTML;
    expect(picked).toContain("hsl(25 65% 48%)");
  });

  it("initials take the first letters of two words, or two letters of one", () => {
    expect(initials("Release notes writer")).toBe("RN");
    expect(initials("Scout")).toBe("SC");
    expect(initials("  ")).toBe("?");
  });
});
