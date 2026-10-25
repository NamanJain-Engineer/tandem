// D-2603 (M18 · Developer-first UI): Agent mode's first run offers one
// primary action and the local templates, each opening New teammate filled in.
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { UiProvider } from "@tandem/ui";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { useNewTeammate } from "./NewTeammateSheet";
import { FirstRun } from "./TeammatePage";

const TEMPLATES = [
  { id: "tpl_reviewer", name: "Code reviewer", brief: "Reviews every diff before it lands.\nBe strict." },
  { id: "tpl_triage", name: "Issue triager", brief: "Sorts new issues." },
];

const show = () =>
  render(
    <UiProvider>
      <FirstRun />
    </UiProvider>,
  );

describe("D-2603 Agent mode's first run", () => {
  beforeEach(() => {
    resetStores();
    useNewTeammate.setState({ open: false, prefill: null, template: null });
  });

  it("shows the templates as cards; picking one opens New teammate with it", async () => {
    fakeBackend({ teammate_templates: () => TEMPLATES });
    show();
    expect(screen.getByLabelText("Loading templates")).toBeTruthy();
    const card = await screen.findByTestId("first-run-template-tpl_reviewer");
    expect(card.textContent).toContain("Reviews every diff before it lands.");
    expect(card.textContent).not.toContain("Be strict.");
    fireEvent.click(card);
    expect(useNewTeammate.getState()).toMatchObject({ open: true, template: TEMPLATES[0] });
  });

  it("the primary action opens an empty New teammate, and stands alone when templates fail", async () => {
    fakeBackend({
      teammate_templates: () => {
        throw { code: "internal", message: "nope" };
      },
    });
    show();
    await waitFor(() => expect(screen.queryByLabelText("Loading templates")).toBeNull());
    expect(screen.queryByTestId(/first-run-template-/)).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "New teammate" }));
    expect(useNewTeammate.getState()).toMatchObject({ open: true, template: null });
  });
});
