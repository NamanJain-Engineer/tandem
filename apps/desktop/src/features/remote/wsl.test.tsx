import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import type { Distro, HostView, IpcError } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { useRemote } from "../../stores/remote";
import { fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { RemoteHosts } from "./RemoteHosts";
import { WslDistros } from "./WslDistros";

const DISTROS: Distro[] = [
  { name: "Ubuntu-22.04", isDefault: true, version: 2 },
  { name: "Debian", isDefault: false, version: 1 },
];

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

function setup(distros: () => Distro[]) {
  calls = fakeBackend({
    wsl_distros: distros,
    workspace_add: (a) => {
      if (a?.root === "/nope") throw { code: "not_found", message: "That folder doesn't exist in Debian.", stage: "folder_missing" } as IpcError;
      return { workspace: { id: "ws_9" }, tab: { id: "tab_9" } };
    },
  });
  return render(
    <UiProvider>
      <WslDistros />
    </UiProvider>,
  );
}

describe("Settings › Remote › WSL (F-RM-02)", () => {
  beforeEach(() => {
    resetStores();
    useRemote.setState({ hosts: [], loaded: false, error: null, attempts: {}, ask: null, engines: {} });
  });

  it("F-RM-02 not on Windows: the WSL section isn't shown", async () => {
    setup(() => {
      throw { code: "unavailable", message: "WSL folders open on Windows only", stage: "wsl" } as IpcError;
    });
    await waitFor(() => expect(named("wsl_distros").length).toBe(1));
    await waitFor(() => expect(screen.queryByTestId("wsl-distros")).toBeNull());
  });

  it("F-RM-02 empty and error states", async () => {
    const { unmount } = setup(() => []);
    expect(await screen.findByText("No WSL distributions")).toBeTruthy();
    unmount();
    setup(() => {
      throw { code: "unavailable", message: "Couldn't run wsl.exe: access denied", stage: "wsl_exe" } as IpcError;
    });
    expect(await screen.findByText("Couldn't run wsl.exe: access denied")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Retry" })).toBeTruthy();
  });

  it("F-RM-02 lists distros with their WSL version and the default", async () => {
    setup(() => DISTROS);
    const rows = await screen.findAllByTestId("wsl-distro");
    expect(rows.map((r) => r.dataset.name)).toEqual(["Ubuntu-22.04", "Debian"]);
    expect(screen.getByText("WSL 2")).toBeTruthy();
    expect(screen.getByText("WSL default")).toBeTruthy();
  });

  it("F-RM-02 'Use for plain terminals' saves wsl_default_distro", async () => {
    setup(() => DISTROS);
    fireEvent.click(await screen.findByRole("radio", { name: "Debian" }));
    await waitFor(() => expect(named("settings_set").at(-1)?.args).toMatchObject({ key: "wsl_default_distro", value: "Debian" }));
    fireEvent.click(screen.getByRole("radio", { name: "Windows (your default shell)" }));
    await waitFor(() => expect(named("settings_set").at(-1)?.args).toMatchObject({ key: "wsl_default_distro", value: null }));
  });

  it("F-RM-02 Open WSL folder adds a wsl workspace in that distro, and shows the core's refusal", async () => {
    setup(() => DISTROS);
    const opens = await screen.findAllByRole("button", { name: "Open WSL folder…" });
    fireEvent.click(opens[1]!);
    const input = await screen.findByTestId("wsl-folder-path");
    fireEvent.change(input, { target: { value: "/nope" } });
    fireEvent.click(screen.getByTestId("wsl-folder-open"));
    expect(await screen.findByText("That folder doesn't exist in Debian.")).toBeTruthy();
    fireEvent.change(input, { target: { value: "/home/ana/shop" } });
    fireEvent.click(screen.getByTestId("wsl-folder-open"));
    await waitFor(() =>
      expect(named("workspace_add").at(-1)?.args).toMatchObject({ kind: "wsl", root: "/home/ana/shop", wslDistro: "Debian" }),
    );
  });

  it("F-RM-02 WSL hosts aren't listed as SSH hosts", async () => {
    const wslHost: HostView = {
      id: "host_wsl",
      kind: "wsl",
      label: "Ubuntu-22.04",
      sshTarget: null,
      sshPort: null,
      identityFile: null,
      wslDistro: "Ubuntu-22.04",
      hostKeyFp: null,
      hostProtocol: null,
      arch: null,
      workspaces: 1,
      createdAt: 0,
      state: "down",
    };
    calls = fakeBackend({ ssh_hosts_list: () => [wslHost] });
    render(
      <UiProvider>
        <RemoteHosts />
      </UiProvider>,
    );
    expect(await screen.findByText("No remote hosts yet")).toBeTruthy();
    expect(screen.queryAllByTestId("remote-host")).toHaveLength(0);
  });
});
