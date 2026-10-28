import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AgentsMdPlan, RunPlan, RunRuleRepo, SecretInfo } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useSecrets } from "../../stores/secrets";
import { fakeBackend, resetStores } from "../../test/fakeBackend";
import { RunDialog } from "./RunDialog";
import { SecretsPage } from "./SecretsPage";
import { TellAgents } from "./TellAgents";

beforeEach(() => {
  resetStores();
  useSecrets.setState({ list: [], state: "idle", error: null });
});
afterEach(() => setInvokeBackend(null));

/** A backend holding secrets by name; values only go in. */
function secretsBackend() {
  const values = new Map<string, string>();
  const calls = fakeBackend({
    secrets_list: () =>
      [...values.keys()].sort().map((name): SecretInfo => ({ name, updatedAt: 1_760_000_000_000 })),
    secrets_set: (a) => {
      values.set(String(a?.name), String(a?.value));
      return { name: String(a?.name), updatedAt: 1 };
    },
    secrets_delete: (a) => {
      values.delete(String(a?.name));
      return null;
    },
  });
  return { values, calls };
}

const page = () =>
  render(
    <UiProvider>
      <SecretsPage />
    </UiProvider>,
  );

describe("agent-proof secrets (F-TL-12)", () => {
  it("F-TL-12.1 the empty page explains, and an added secret shows its reference, never its value", async () => {
    const { values } = secretsBackend();
    page();
    expect((await screen.findByText("No secrets yet")).textContent).toBe("No secrets yet");
    fireEvent.click(screen.getByTestId("secret-add"));
    fireEvent.change(await screen.findByTestId("secret-name"), { target: { value: "stripe-test" } });
    const value = screen.getByTestId("secret-value") as HTMLInputElement;
    expect(value.type).toBe("password");
    fireEvent.change(value, { target: { value: "sk_test_42" } });
    fireEvent.click(screen.getByTestId("secret-save"));
    const row = await screen.findByTestId("secret-row");
    expect(row.dataset.name).toBe("stripe-test");
    expect(screen.getByTestId("secret-reference").textContent).toBe("vibezeo-secret://stripe-test");
    expect(document.body.textContent).not.toContain("sk_test_42");
    expect(values.get("stripe-test")).toBe("sk_test_42");
  });

  it("F-TL-12.1 a name that breaks the rules can't be saved", async () => {
    secretsBackend();
    page();
    fireEvent.click(await screen.findByTestId("secret-add"));
    fireEvent.change(await screen.findByTestId("secret-name"), { target: { value: "Stripe Key" } });
    fireEvent.change(screen.getByTestId("secret-value"), { target: { value: "x" } });
    expect((screen.getByTestId("secret-save") as HTMLButtonElement).disabled).toBe(true);
  });

  it("F-TL-12.3 a value too short to mask is called out", async () => {
    secretsBackend();
    page();
    fireEvent.click(await screen.findByTestId("secret-add"));
    fireEvent.change(await screen.findByTestId("secret-value"), { target: { value: "abc" } });
    expect(
      screen.getByText("Values under 4 bytes aren't hidden in the pane: they'd match ordinary output."),
    ).toBeTruthy();
    fireEvent.change(screen.getByTestId("secret-value"), { target: { value: "abcd" } });
    expect(screen.queryByText(/Values under 4 bytes/)).toBeNull();
  });

  it("F-TL-12.6 Delete asks first, then removes it", async () => {
    const { values } = secretsBackend();
    values.set("db", "postgres://x");
    page();
    fireEvent.click(await screen.findByTestId("secret-delete"));
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByTestId("secret-row")).toBeNull());
    expect(values.has("db")).toBe(false);
  });

  it("the keychain's error shows with Retry", async () => {
    fakeBackend({
      secrets_list: () => {
        throw { message: "Your system keyring is locked or missing", code: "unavailable" };
      },
    });
    page();
    expect((await screen.findByTestId("secrets-error")).textContent).toBe(
      "Your system keyring is locked or missing",
    );
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });

  const plan = (
    vars: RunPlan["vars"],
    envFile: string | null = "/w/checkout/.env",
    unavailable: string | null = null,
  ): RunPlan => ({
    folder: "/w/checkout",
    envFile,
    vars,
    unavailable,
  });

  const dialog = () =>
    render(
      <UiProvider>
        <RunDialog paneId="pn_1" tabId="tb_1" open onOpenChange={() => undefined} />
      </UiProvider>,
    );

  it("F-TL-12.2 Run shows what it uses and starts the command in a new pane", async () => {
    const calls = fakeBackend({
      secrets_run_plan: () =>
        plan([
          { name: "API_KEY", secret: "stripe-test", missing: false, malformed: false },
          { name: "PORT", secret: null, missing: false, malformed: false },
        ]),
      secrets_run: () => ({ id: "pn_2", tabId: "tb_1", kind: "terminal" }),
    });
    dialog();
    expect((await screen.findByTestId("run-plan")).textContent).toContain("Uses .env in checkout");
    expect(screen.getAllByTestId("run-var").map((v) => v.textContent)).toEqual([
      "API_KEY → stripe-test",
      "PORT",
    ]);
    const start = screen.getByTestId("run-start") as HTMLButtonElement;
    expect(start.disabled).toBe(true);
    fireEvent.change(screen.getByTestId("run-command"), { target: { value: "pnpm dev" } });
    fireEvent.click(start);
    await waitFor(() => expect(calls.some((c) => c.cmd === "secrets_run")).toBe(true));
    expect(calls.find((c) => c.cmd === "secrets_run")?.args).toEqual({
      paneId: "pn_1",
      tabId: "tb_1",
      command: "pnpm dev",
    });
  });

  it("F-TL-12.4 a missing secret is named and Run stays disabled", async () => {
    fakeBackend({
      secrets_run_plan: () => plan([{ name: "API_KEY", secret: "gone", missing: true, malformed: false }]),
    });
    dialog();
    const v = await screen.findByTestId("run-var");
    expect(v.dataset.missing).toBe("true");
    expect(v.textContent).toContain("No secret named gone");
    fireEvent.change(screen.getByTestId("run-command"), { target: { value: "pnpm dev" } });
    expect((screen.getByTestId("run-start") as HTMLButtonElement).disabled).toBe(true);
  });

  it("F-TL-12.4 a malformed reference is flagged and Run stays disabled", async () => {
    fakeBackend({
      secrets_run_plan: () => plan([{ name: "API_KEY", secret: null, missing: false, malformed: true }]),
    });
    dialog();
    const v = await screen.findByTestId("run-var");
    expect(v.dataset.malformed).toBe("true");
    expect(v.textContent).toContain("Not a valid vibezeo-secret:// reference");
    fireEvent.change(screen.getByTestId("run-command"), { target: { value: "pnpm dev" } });
    expect((screen.getByTestId("run-start") as HTMLButtonElement).disabled).toBe(true);
  });

  it("F-TL-12.5 where the shell can't keep the values to the command, Run says why and stays disabled", async () => {
    fakeBackend({
      secrets_run_plan: () => plan([], null, "Run with secrets needs a POSIX shell, fish or PowerShell"),
    });
    dialog();
    expect(await screen.findByText("Run with secrets needs a POSIX shell, fish or PowerShell")).toBeTruthy();
    fireEvent.change(screen.getByTestId("run-command"), { target: { value: "pnpm dev" } });
    expect((screen.getByTestId("run-start") as HTMLButtonElement).disabled).toBe(true);
  });

  it("without a .env, it says so and runs the command as typed", async () => {
    fakeBackend({ secrets_run_plan: () => plan([], null) });
    dialog();
    expect((await screen.findByTestId("run-no-env")).textContent).toBe(
      "No .env in this folder. The command runs as typed.",
    );
  });
});

describe("vibezeo run for agents (F-TL-12 slice 2)", () => {
  it("F-TL-12.9 a folder's rules are listed, added and removed; a refused rule says why", async () => {
    let repo: RunRuleRepo = { repo: "/w/shop", name: "shop", commands: ["pnpm deploy"] };
    const calls = fakeBackend({
      secrets_list: () => [],
      run_rules_list: () => [repo],
      run_rules_set: (a) => {
        const commands = (a?.commands as string[]) ?? [];
        if (commands.some((c) => c.includes(";")))
          throw { code: "invalid", message: "A rule can't hold ; & | < > $ ` ( ) or line breaks" };
        repo = { ...repo, commands };
        return repo;
      },
    });
    render(
      <UiProvider>
        <SecretsPage />
      </UiProvider>,
    );
    const row = await screen.findByTestId("run-rules-repo");
    expect(row.textContent).toContain("shop");
    expect(row.textContent).toContain("pnpm deploy");
    fireEvent.change(screen.getByTestId("run-rules-input"), { target: { value: "make; ls" } });
    fireEvent.click(screen.getByTestId("run-rules-add"));
    expect(await screen.findByText("A rule can't hold ; & | < > $ ` ( ) or line breaks")).toBeTruthy();
    fireEvent.change(screen.getByTestId("run-rules-input"), { target: { value: "pnpm test" } });
    fireEvent.click(screen.getByTestId("run-rules-add"));
    await waitFor(() => expect(screen.getByTestId("run-rules-commands").textContent).toContain("pnpm test"));
    expect(calls.filter((c) => c.cmd === "run_rules_set").pop()?.args).toEqual({
      repo: "/w/shop",
      commands: ["pnpm deploy", "pnpm test"],
    });
    fireEvent.click(screen.getByRole("button", { name: "Remove pnpm deploy" }));
    await waitFor(() =>
      expect(screen.getByTestId("run-rules-commands").textContent).not.toContain("pnpm deploy"),
    );
  });

  it("without folders, the rules say how to get one", async () => {
    fakeBackend({ secrets_list: () => [], run_rules_list: () => [] });
    render(
      <UiProvider>
        <SecretsPage />
      </UiProvider>,
    );
    expect((await screen.findByTestId("run-rules-empty")).textContent).toBe(
      "Add a folder to Vibezeo to set rules for it.",
    );
  });

  const tell = () =>
    render(
      <UiProvider>
        <TellAgents paneId="pn_1" open onOpenChange={() => undefined} />
      </UiProvider>,
    );

  it("Tell agents shows the AGENTS.md change first and writes what was shown", async () => {
    const plan: AgentsMdPlan = {
      path: "/w/shop/AGENTS.md",
      before: "# Mine\n",
      after: "# Mine\n\n<!-- vibezeo:run -->\nCommands that need secrets…\n<!-- /vibezeo:run -->\n",
      patch:
        "diff --git a/AGENTS.md b/AGENTS.md\n--- a/AGENTS.md\n+++ b/AGENTS.md\n@@ -1,1 +1,5 @@\n # Mine\n+\n+<!-- vibezeo:run -->\n+Commands that need secrets…\n+<!-- /vibezeo:run -->\n",
      unchanged: false,
    };
    const calls = fakeBackend({
      secrets_agents_plan: () => plan,
      secrets_agents_apply: () => ({ ...plan, before: plan.after, patch: "", unchanged: true }),
    });
    tell();
    expect((await screen.findByTestId("diff-view")).textContent).toContain("Commands that need secrets…");
    fireEvent.click(screen.getByTestId("tell-agents-apply"));
    await waitFor(() => expect(calls.some((c) => c.cmd === "secrets_agents_apply")).toBe(true));
    expect(calls.find((c) => c.cmd === "secrets_agents_apply")?.args).toEqual({
      paneId: "pn_1",
      before: "# Mine\n",
    });
  });

  it("Tell agents says so when AGENTS.md already has it, and a changed file is refused", async () => {
    fakeBackend({
      secrets_agents_plan: () => ({
        path: "/w/AGENTS.md",
        before: "x",
        after: "x",
        patch: "",
        unchanged: true,
      }),
    });
    tell();
    expect(await screen.findByTestId("tell-agents-unchanged")).toBeTruthy();
    expect((screen.getByTestId("tell-agents-apply") as HTMLButtonElement).disabled).toBe(true);
  });
});
