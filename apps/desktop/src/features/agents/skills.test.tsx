import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { SkillEntry, Teammate } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { useAccounts } from "../../stores/accounts";
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

describe("F-AG-10 Also install for an engine", () => {
  let skills: SkillEntry[];
  let calls: Call[];
  let outcome: "synced" | "removed" | "conflict";
  beforeEach(() => {
    resetStores();
    useAccounts.setState({ list: [], loaded: false, failed: false });
    skills = [entry("release-notes"), entry("its-own", { isNew: true })];
    outcome = "synced";
    calls = fakeBackend({
      skills_list: () => skills,
      engines_list: () => [
        { spec: { id: "claude", label: "Claude Code", logo: "CC", skillsDir: "skills" }, install: { path: "/bin/claude" } },
        { spec: { id: "aider", label: "Aider", logo: "Ai", skillsDir: null }, install: { path: "/bin/aider" } },
        { spec: { id: "codex", label: "Codex", logo: "Cx", skillsDir: "skills" }, install: null },
      ],
      engines_report: () => null,
      accounts_list: () => [
        { id: "acc_c", engineId: "claude", label: "Personal", homePath: "/p/claude", isDefault: true },
        { id: "acc_a", engineId: "aider", label: "Personal", homePath: "/p/aider", isDefault: true },
      ],
      accounts_overrides: () => [],
      skill_sync: (a) => {
        if (outcome === "synced") skills = skills.map((x) => (x.slug === a?.slug ? { ...x, syncedTo: ["acc_c"] } : x));
        if (outcome === "removed") skills = skills.map((x) => (x.slug === a?.slug ? { ...x, syncedTo: [] } : x));
        return { state: outcome };
      },
    });
    render(
      <UiProvider>
        <SkillsTab teammate={SCOUT} />
      </UiProvider>,
    );
  });

  const openMenu = async (slug: string) => {
    const row = (await screen.findAllByTestId("skill-row")).find((r) => r.dataset.slug === slug) as HTMLElement;
    const trigger = within(row).getByRole("button", { name: `Actions for ${slug}` });
    fireEvent.keyDown(trigger, { key: "Enter" });
    await screen.findByRole("menuitem", { name: "Edit" });
    return row;
  };

  it("F-AG-10.1 syncing to Claude marks the skill installed, and turning it off removes it", async () => {
    await openMenu("release-notes");
    // Only installed engines with a skills folder are offered.
    expect(screen.queryByRole("menuitemcheckbox", { name: "Aider" })).toBeNull();
    expect(screen.queryByRole("menuitemcheckbox", { name: "Codex" })).toBeNull();
    await act(async () => fireEvent.click(await screen.findByRole("menuitemcheckbox", { name: "Claude Code" })));
    const sync = calls.find((c) => c.cmd === "skill_sync");
    expect(sync?.args).toEqual({ teammateId: "tm_scout", slug: "release-notes", engineId: "claude", accountId: "acc_c", on: true });
    await waitFor(() => expect(screen.getByText("In Claude Code")).toBeTruthy());
    outcome = "removed";
    await openMenu("release-notes");
    const box = await screen.findByRole("menuitemcheckbox", { name: "Claude Code" });
    expect(box.getAttribute("aria-checked")).toBe("true");
    await act(async () => fireEvent.click(box));
    expect(calls.filter((c) => c.cmd === "skill_sync").at(-1)?.args?.on).toBe(false);
    await waitFor(() => expect(screen.queryByText("In Claude Code")).toBeNull());
  });

  it("F-AG-10.2 a same-name skill the engine already has is a conflict, shown on the row", async () => {
    outcome = "conflict";
    const row = await openMenu("release-notes");
    await act(async () => fireEvent.click(await screen.findByRole("menuitemcheckbox", { name: "Claude Code" })));
    await waitFor(() =>
      expect(within(row).getByTestId("skill-conflict").textContent).toBe(
        "Claude Code already has its own skill called release-notes. Vibezeo left it as it is and didn't install this one.",
      ),
    );
    expect(within(row).queryByText("In Claude Code")).toBeNull();
  });

  it("F-AG-10 a teammate's new skill can't be installed until it's opened", async () => {
    await openMenu("its-own");
    const box = await screen.findByRole("menuitemcheckbox", { name: "Claude Code" });
    expect(box.getAttribute("aria-disabled") ?? box.getAttribute("data-disabled")).not.toBeNull();
  });
});
