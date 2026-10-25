import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { AcpAgent, EngineView } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { FREE, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { engineOptionsFor } from "../thread/ThreadComposer";
import { ownSignIn } from "../thread/ThreadMain";
import { AcpAgents } from "./AcpAgents";

const GOOSE: AcpAgent = {
  id: "acp-goose",
  label: "Goose",
  program: "/usr/local/bin/goose",
  args: ["acp"],
  env: {},
};

let calls: Call[];
function open(handlers: Record<string, (a?: Record<string, unknown>) => unknown>) {
  calls = fakeBackend({ engines_list: () => [], engines_report: () => null, ...handlers });
  render(
    <UiProvider>
      <AcpAgents />
    </UiProvider>,
  );
}

const type = (testId: string, value: string) =>
  fireEvent.change(screen.getByTestId(testId), { target: { value } });

const goose: EngineView = {
  spec: { id: "acp-goose-0000002a", label: "Goose", logo: "GO" } as EngineView["spec"],
  install: { path: "/usr/local/bin/goose", version: null, source: "shell" },
  chatCapable: true,
  unsupportedReason: null,
  extraArgs: [],
  added: true,
};

describe("UI-163 ACP agents (F-EN-10)", () => {
  beforeEach(() => resetStores());

  it("F-EN-10.1 Test shows the agent's answer, and Add saves it with its arguments and variables", async () => {
    const agents: AcpAgent[] = [];
    open({
      acp_agents_list: () => agents,
      acp_agent_test: () => ({
        name: "Goose",
        version: "1.9.0",
        protocolVersion: 1,
        authMethods: ["Sign in with Block"],
      }),
      acp_agent_add: (a) => {
        agents.push(GOOSE);
        return { ...GOOSE, env: (a?.input as { env: Record<string, string> }).env };
      },
    });
    expect(await screen.findByTestId("acp-empty")).toBeTruthy();
    fireEvent.click(screen.getByTestId("acp-add-open"));
    fireEvent.click(await screen.findByTestId("acp-preset-goose"));
    expect((screen.getByTestId("acp-command") as HTMLInputElement).value).toBe("goose");
    fireEvent.click(screen.getByRole("button", { name: "Add a variable" }));
    fireEvent.change(screen.getByPlaceholderText("NAME"), { target: { value: "GOOSE_MODE" } });
    fireEvent.change(screen.getByPlaceholderText("value"), { target: { value: "auto" } });
    await act(async () => fireEvent.click(screen.getByTestId("acp-test")));
    expect((await screen.findByTestId("acp-tested")).textContent).toBe(
      "Goose answered, version 1.9.0. It signs in with: Sign in with Block.",
    );
    expect(calls.find((c) => c.cmd === "acp_agent_test")?.args).toEqual({
      input: { label: "Goose", command: "goose", args: ["acp"], env: { GOOSE_MODE: "auto" } },
    });
    await act(async () => fireEvent.click(screen.getByTestId("acp-add-confirm")));
    await waitFor(() => expect(screen.getAllByTestId("acp-agent")).toHaveLength(1));
    expect(screen.queryByTestId("acp-add")).toBeNull();
    // The engine list is read again, so the picker has the new agent.
    expect(calls.some((c) => c.cmd === "engines_list")).toBe(true);
  });

  it("F-EN-10.2 a command that isn't an ACP agent says so, and nothing is saved", async () => {
    open({
      acp_agents_list: () => [],
      acp_agent_add: () => {
        throw { code: "unavailable", message: "Not an ACP agent: it didn't answer within 10 seconds" };
      },
    });
    fireEvent.click(await screen.findByTestId("acp-add-open"));
    type("acp-label", "Sleepy");
    type("acp-command", "/bin/sleep");
    type("acp-args", "30");
    await act(async () => fireEvent.click(screen.getByTestId("acp-add-confirm")));
    expect((await screen.findByTestId("acp-error")).textContent).toBe(
      "Not an ACP agent: it didn't answer within 10 seconds",
    );
    expect(screen.getByTestId("acp-add")).toBeTruthy();
    expect(screen.getByTestId("acp-empty")).toBeTruthy();
  });

  it("F-EN-10.4 a refused variable's reason shows in the form", async () => {
    open({
      acp_agents_list: () => [],
      acp_agent_test: () => {
        throw {
          code: "invalid",
          message: "OPENAI_API_KEY looks like a key or token. Keep keys in the agent's own sign-in",
        };
      },
    });
    fireEvent.click(await screen.findByTestId("acp-add-open"));
    type("acp-label", "Agent");
    type("acp-command", "agent");
    await act(async () => fireEvent.click(screen.getByTestId("acp-test")));
    expect((await screen.findByTestId("acp-error")).textContent).toContain(
      "Keep keys in the agent's own sign-in",
    );
    // An unclosed quote never reaches the core.
    type("acp-args", '"open');
    expect((screen.getByTestId("acp-test") as HTMLButtonElement).disabled).toBe(true);
  });

  it("UI-163 Remove asks first, and a load error offers Try again", async () => {
    let fail = true;
    open({
      acp_agents_list: () => {
        if (fail) throw { code: "io", message: "Couldn't read the agents" };
        return [GOOSE];
      },
      acp_agent_remove: () => null,
    });
    expect(await screen.findByText("Couldn't read the agents")).toBeTruthy();
    fail = false;
    await act(async () => fireEvent.click(screen.getByRole("button", { name: "Try again" })));
    const row = await screen.findByTestId("acp-agent");
    fireEvent.click(within(row).getByRole("button", { name: "Remove" }));
    expect(calls.some((c) => c.cmd === "acp_agent_remove")).toBe(false);
    const dialog = await screen.findByRole("alertdialog");
    await act(async () => fireEvent.click(within(dialog).getByRole("button", { name: "Remove" })));
    expect(calls.find((c) => c.cmd === "acp_agent_remove")?.args).toEqual({ id: "acp-goose" });
  });

  it('F-EN-10.3 an added agent reads "via ACP" in the picker, enabled with no account (D-869)', () => {
    const free = engineOptionsFor([goose], FREE.chatEngines);
    expect(FREE.chatEngines).toBeNull();
    expect(free[0]).toMatchObject({ label: "Goose · via ACP", disabled: false });
  });

  it("F-EN-10.1 an added agent's sign-in is its own, never an account's (D-2514)", () => {
    expect(ownSignIn(goose)).toBe(
      "Sign in to Goose with its own command in a terminal, then try again. Vibezeo never holds its sign-in.",
    );
    expect(ownSignIn({ ...goose, added: false })).toBeNull();
    expect(ownSignIn(undefined)).toBeNull();
  });
});
