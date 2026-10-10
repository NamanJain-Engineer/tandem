import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ContextMeter, tokenCount } from "./Progress";

describe("ContextMeter (F-AC-06)", () => {
  it("F-AC-06.1 150k of 200k is 75% and neutral; 170k is amber", () => {
    const { rerender } = render(<ContextMeter label="Context" used={150_000} window={200_000} />);
    const meter = screen.getByRole("meter", { name: "Context" });
    expect(meter.getAttribute("aria-valuenow")).toBe("75");
    expect(meter.textContent).toBe("150K / 200K");
    expect(meter.dataset.tone).toBe("muted");
    rerender(<ContextMeter label="Context" used={170_000} window={200_000} />);
    expect(screen.getByRole("meter", { name: "Context" }).dataset.tone).toBe("warning");
  });

  it("F-AC-06 token counts read as K and M", () => {
    expect(tokenCount(950)).toBe("950");
    expect(tokenCount(114_400)).toBe("114K");
    expect(tokenCount(1_000_000)).toBe("1M");
    expect(tokenCount(1_200_000)).toBe("1.2M");
  });
});
