import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { HostView, IpcError } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { useRemote, followRemote } from "../../stores/remote";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { RemoteHosts } from "./RemoteHosts";
import { RemotePrompts } from "./RemotePrompts";

const HOST: HostView = {
  id: "host_01",
  kind: "ssh",
  label: "Build box",
  sshTarget: "tandem@10.0.0.5",
  sshPort: 2222,
  identityFile: null,
  wslDistro: null,
  hostKeyFp: "SHA256:old",
  hostProtocol: 1,
  arch: "linux-x86_64",
  workspaces: 1,
  createdAt: 0,
  state: "down",
};

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

function setup(hosts: HostView[] | (() => HostView[])) {
  calls = fakeBackend({
    ssh_hosts_list: () => (typeof hosts === "function" ? hosts() : hosts),
    ssh_connect: () => ({ opId: "op_1" }),
    ssh_host_add: (a) => ({ ...HOST, id: "host_02", sshTarget: String(a?.target) }),
    ssh_list_dir: (a) => ({
      path: a?.path === "" ? "/home/tandem" : String(a?.path),
      parent: "/home",
      dirs: ["project", ".config"],
    }),
    engines_list: () => [],
  });
  return render(
    <UiProvider>
      <RemoteHosts />
      <RemotePrompts />
    </UiProvider>,
  );
}

describe("Settings › Remote (UI-14, UI-111)", () => {
  beforeEach(() => {
    resetStores();
    useRemote.setState({ hosts: [], loaded: false, error: null, attempts: {}, ask: null, engines: {} });
  });

  it("F-RM-01 empty: says what remote hosts are and offers Add host", async () => {
    setup([]);
    expect(await screen.findByText("No remote hosts yet")).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Add host" }).length).toBeGreaterThan(0);
  });

  it("F-RM-01 error: a failed list says so", async () => {
    calls = fakeBackend({
      ssh_hosts_list: () => {
        throw { code: "internal", message: "The database is busy" } as IpcError;
      },
    });
    render(
      <UiProvider>
        <RemoteHosts />
      </UiProvider>,
    );
    expect(await screen.findByText("The database is busy")).toBeTruthy();
  });

  it("F-RM-01 populated: user@host:port, workspaces, state and Connect", async () => {
    setup([HOST]);
    expect(await screen.findByText("Build box")).toBeTruthy();
    expect(screen.getByText("tandem@10.0.0.5:2222")).toBeTruthy();
    expect(screen.getByText("1 workspace")).toBeTruthy();
    expect(screen.getByText("Disconnected")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Connect" }));
    await waitFor(() =>
      expect(named("ssh_connect")).toEqual([{ cmd: "ssh_connect", args: { hostId: "host_01" } }]),
    );
  });

  it("F-RM-01 Add host sends the target, user, port and key file, then connects", async () => {
    setup([]);
    fireEvent.click((await screen.findAllByRole("button", { name: "Add host" }))[0]!);
    fireEvent.change(await screen.findByLabelText("Host"), { target: { value: "10.0.0.9" } });
    fireEvent.change(screen.getByLabelText("User"), { target: { value: "ana" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "2200" } });
    fireEvent.click(screen.getByRole("radio", { name: "Key file…" }));
    fireEvent.change(screen.getByLabelText("Key file"), { target: { value: "/home/me/.ssh/id_ed25519" } });
    fireEvent.click(screen.getByRole("button", { name: "Save and connect" }));
    await waitFor(() =>
      expect(named("ssh_host_add")[0]?.args).toEqual({
        target: "10.0.0.9",
        user: "ana",
        port: 2200,
        identityFile: "/home/me/.ssh/id_ed25519",
        label: null,
      }),
    );
    await waitFor(() => expect(named("ssh_connect").length).toBe(1));
  });

  it("F-RM-01 a port out of range can't be saved", async () => {
    setup([]);
    fireEvent.click((await screen.findAllByRole("button", { name: "Add host" }))[0]!);
    fireEvent.change(await screen.findByLabelText("Host"), { target: { value: "h" } });
    fireEvent.change(screen.getByLabelText("Port"), { target: { value: "70000" } });
    expect(screen.getByText("Use a port from 1 to 65535")).toBeTruthy();
    expect((screen.getByRole("button", { name: "Save and connect" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  it("F-RM-01.2 a changed host key shows the warning and Trust new key…", async () => {
    setup([HOST]);
    followRemote();
    await screen.findByText("Build box");
    act(() => {
      emitLocal("remote:ssh", {
        opId: "op_1",
        hostId: "host_01",
        stage: "failed",
        prompt: null,
        error: {
          code: "unavailable",
          message:
            "The host key for tandem@10.0.0.5:2222 changed. Vibezeo won't connect until you verify it.",
          stage: "host_key",
          hint: null,
          retryable: false,
        },
      });
    });
    expect(
      await screen.findByText("Host key changed for Build box. Vibezeo won't connect until you verify it."),
    ).toBeTruthy();
    expect(screen.getByText("Pinned key: SHA256:old")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Trust new key…" }));
    fireEvent.click(await screen.findByRole("button", { name: "Forget the old key" }));
    await waitFor(() =>
      expect(named("ssh_host_reset_key")).toEqual([
        { cmd: "ssh_host_reset_key", args: { hostId: "host_01" } },
      ]),
    );
  });

  it("F-RM-01 an unknown host's fingerprint is shown and the answer goes back", async () => {
    setup([HOST]);
    followRemote();
    act(() => {
      emitLocal("remote:ssh", {
        opId: "op_7",
        hostId: "host_01",
        stage: "hostkey",
        prompt: {
          kind: "hostkey",
          host: "tandem@10.0.0.5:2222",
          fingerprint: "SHA256:abc",
          keyType: "ssh-ed25519",
        },
        error: null,
      });
    });
    expect(await screen.findByText("SHA256:abc")).toBeTruthy();
    // It can open on its own while connecting: Trust and connect waits a
    // moment first, so a click aimed at what was there trusts nothing.
    const trust = screen.getByRole("button", { name: "Trust and connect" }) as HTMLButtonElement;
    expect(trust.disabled).toBe(true);
    expect(trust.title).toBe("Trust and connect turns on once this has been still on screen for a moment.");
    fireEvent.click(trust);
    expect(named("ssh_hostkey_decide")).toEqual([]);
    await waitFor(() => expect(trust.disabled).toBe(false));
    fireEvent.click(trust);
    await waitFor(() =>
      expect(named("ssh_hostkey_decide")).toEqual([
        { cmd: "ssh_hostkey_decide", args: { opId: "op_7", accept: true } },
      ]),
    );
  });

  it("F-RM-01 the password prompt sends the secret to ssh_askpass_reply only", async () => {
    setup([HOST]);
    followRemote();
    act(() => {
      emitLocal("remote:ssh", {
        opId: "op_8",
        hostId: "host_01",
        stage: "password",
        prompt: { kind: "password", label: "tandem@10.0.0.5's password:" },
        error: null,
      });
    });
    fireEvent.change(await screen.findByLabelText("tandem@10.0.0.5's password:"), {
      target: { value: "s3cret" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() =>
      expect(named("ssh_askpass_reply")).toEqual([
        { cmd: "ssh_askpass_reply", args: { opId: "op_8", secret: "s3cret" } },
      ]),
    );
    expect(useRemote.getState().ask).toBeNull();
  });

  it("F-RM-01 a slow first listing doesn't replace the folder typed meanwhile", async () => {
    let home: (v: unknown) => void = () => {};
    calls = fakeBackend({
      ssh_hosts_list: () => [{ ...HOST, state: "connected" }],
      ssh_list_dir: (a) =>
        a?.path === ""
          ? new Promise((r) => (home = r))
          : { path: String(a?.path), parent: "/home/tandem", dirs: [] },
      engines_list: () => [],
    });
    render(
      <UiProvider>
        <RemoteHosts />
        <RemotePrompts />
      </UiProvider>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Open folder…" }));
    const field = await screen.findByTestId("remote-picker-path");
    fireEvent.change(field, { target: { value: "/home/tandem/project" } });
    fireEvent.submit(field.closest("form") as HTMLFormElement);
    await waitFor(() =>
      expect(screen.getByTestId("remote-picker").getAttribute("data-path")).toBe("/home/tandem/project"),
    );
    // Home answers last (a fresh helper's first call); the typed folder stays.
    home({ path: "/home/tandem", parent: "/home", dirs: ["project"] });
    await new Promise((r) => setTimeout(r, 20));
    expect(screen.getByTestId("remote-picker").getAttribute("data-path")).toBe("/home/tandem/project");
    expect(screen.getByTestId("remote-picker").getAttribute("data-state")).toBe("ready");
    fireEvent.click(screen.getByRole("button", { name: "Open this folder" }));
    await waitFor(() =>
      expect(named("workspace_add")[0]?.args).toMatchObject({ root: "/home/tandem/project" }),
    );
  });

  it("F-RM-01 a connected host opens a folder through the picker", async () => {
    setup([{ ...HOST, state: "connected" }]);
    fireEvent.click(await screen.findByRole("button", { name: "Open folder…" }));
    expect(await screen.findByText("project")).toBeTruthy();
    fireEvent.click(screen.getByText("project"));
    await waitFor(() =>
      expect(named("ssh_list_dir").at(-1)?.args).toEqual({ hostId: "host_01", path: "/home/tandem/project" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Open this folder" }));
    await waitFor(() =>
      expect(named("workspace_add")[0]?.args).toEqual({
        root: "/home/tandem/project",
        kind: "ssh",
        hostId: "host_01",
        wslDistro: null,
      }),
    );
  });
});
