import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { SkillEntry, Teammate } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { SkillsTab } from "./SkillsTab";
import { checklist, joinSkill, splitSkill } from "./skillText";

const SCOUT = { id: "tm_scout", name: "Scout", home: "/h" } as unknown as Teammate;

function entry(slug: string, extra: Partial<SkillEntry> = {}): SkillEntry {
  return {
    slug,
    name: slug,
    description: `Use ${slug}.`,
    valid: true,
    error: null,
    isNew: false,
    syncedTo: [],
    ...extra,
  };
}

describe("skill text", () => {
  it("F-AG-09 fields round-trip through SKILL.md and odd values are quoted", () => {
    const md = joinSkill({
      name: "weekly-deps",
      description: 'Checks: outdated "packages"',
      body: "## Steps\n1. Run it.\n",
    });
    expect(md).toBe(
      '---\nname: weekly-deps\ndescription: "Checks: outdated \\"packages\\""\n---\n## Steps\n1. Run it.\n',
    );
    expect(splitSkill(md)).toEqual({
      name: "weekly-deps",
      description: 'Checks: outdated "packages"',
      body: "## Steps\n1. Run it.\n",
    });
    expect(splitSkill("no front matter")).toBeNull();
    expect(splitSkill("---\nname: a\ndescription: >\n  folded\n---\n")).toBeNull();
  });

  it("UI-74 the checklist reads the body", () => {
    const ok = checklist({
      name: "a",
      description: "d",
      body: "1. Run it.\nVerify the output.\nDon't push.",
    });
    expect(ok.every((c) => c.ok)).toBe(true);
    expect(checklist({ name: "a", description: "", body: "" }).some((c) => c.ok)).toBe(false);
  });
});

describe("UI-74 Skills tab", () => {
  let skills: SkillEntry[];
  let calls: Call[];
  beforeEach(() => {
    resetStores();
    skills = [
      entry("release-notes"),
      entry("broken", { valid: false, error: "The front matter needs a description", description: "" }),
      entry("its-own", { isNew: true }),
    ];
    calls = fakeBackend({
      skills_list: () => skills,
      skills_library: () => [entry("release-notes"), entry("test-first")],
      skill_add_from_library: (a) => {
        skills = [...skills, entry(String(a?.slug))];
        return null;
      },
      skill_get: (a) => ({
        entry: entry(String(a?.slug)),
        markdown: `---\nname: ${String(a?.slug)}\ndescription: Use it.\n---\nbody\n`,
      }),
      skill_save: () => null,
      redact_text: (a) => String(a?.text).replace(/token=\S+/g, "[REDACTED]"),
    });
    render(
      <UiProvider>
        <SkillsTab teammate={SCOUT} />
      </UiProvider>,
    );
  });

  it("F-AG-09.2 an invalid skill shows its error; a teammate's own is New", async () => {
    const rows = await screen.findAllByTestId("skill-row");
    expect(rows.map((r) => r.dataset.slug)).toEqual(["release-notes", "broken", "its-own"]);
    const broken = rows[1] as HTMLElement;
    expect(broken.dataset.valid).toBe("false");
    expect(within(broken).getByText("The front matter needs a description")).toBeTruthy();
    expect(within(rows[2] as HTMLElement).getByText("New")).toBeTruthy();
  });

  it("F-AG-09.1 Add from library adds the skill", async () => {
    await screen.findAllByTestId("skill-row");
    fireEvent.click(screen.getByTestId("skills-library"));
    const add = await screen.findByTestId("library-add-test-first");
    expect((screen.getByTestId("library-add-release-notes") as HTMLButtonElement).disabled).toBe(true);
    await act(async () => fireEvent.click(add));
    expect(calls.some((c) => c.cmd === "skill_add_from_library" && c.args?.slug === "test-first")).toBe(true);
    await waitFor(() => expect(screen.getAllByTestId("skill-row")).toHaveLength(4));
  });

  it("UI-74 a new skill needs a kebab-case name and saves its SKILL.md", async () => {
    await screen.findAllByTestId("skill-row");
    fireEvent.click(screen.getByTestId("skills-new"));
    const name = await screen.findByTestId("skill-name");
    fireEvent.change(name, { target: { value: "Weekly Deps" } });
    await screen.findByText("Use lowercase letters, numbers and dashes.");
    fireEvent.change(name, { target: { value: "weekly-deps" } });
    fireEvent.change(screen.getByTestId("skill-description"), {
      target: { value: "Checks for outdated packages." },
    });
    await waitFor(() => expect((screen.getByTestId("skill-save") as HTMLButtonElement).disabled).toBe(false));
    await act(async () => fireEvent.click(screen.getByTestId("skill-save")));
    const saved = calls.find((c) => c.cmd === "skill_save");
    expect(saved?.args?.slug).toBe("weekly-deps");
    expect(String(saved?.args?.markdown)).toMatch(
      /^---\nname: weekly-deps\ndescription: Checks for outdated packages\.\n---\n/,
    );
  });
});
