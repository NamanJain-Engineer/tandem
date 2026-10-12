import { $, browser } from "@wdio/globals";
import { writeFileSync } from "node:fs";
import { freshOffline, invoke, settled } from "../support/app";
import {
  KEY,
  approveHostKey,
  hostFingerprint,
  hostKeyShown,
  hostRows,
  openRemotePage,
} from "../support/remote";

// E2E-M14-9 Hosts behind a jump host (F-RM-01, D-1603): the user's SSH
// config (here the file TANDEM_SSH_CONFIG names, which the app passes to
// ssh as `-F`) reaches port 22 *inside* the fixture (as `localhost`, and
// as `127.0.0.1` so each host has its own pinned key) through the fixture
// itself, once by ProxyJump and once by ProxyCommand. Nothing
// listens on this machine's port 22, so a direct key scan can't reach it:
// the fingerprint shown is the one ssh saw through the jump, and it's the
// container's own.

const ALIASES = ["via-jump", "via-command"];

function writeConfig(): void {
  const path = process.env.TANDEM_SSH_CONFIG;
  if (!path) throw new Error("TANDEM_SSH_CONFIG isn't set; run through e2e/run.ts");
  const inner = (alias: string, name: string, how: string) =>
    [
      `Host ${alias}`,
      `  HostName ${name}`,
      "  Port 22",
      "  User tandem",
      `  IdentityFile ${KEY}`,
      "  IdentitiesOnly yes",
      `  ${how}`,
    ].join("\n");
  writeFileSync(
    path,
    [
      // The jump host is the user's business: their config, their keys.
      "Host tandem-jump",
      "  HostName 127.0.0.1",
      "  Port 2222",
      "  User tandem",
      `  IdentityFile ${KEY}`,
      "  IdentitiesOnly yes",
      "  StrictHostKeyChecking no",
      "  UserKnownHostsFile /dev/null",
      "  LogLevel ERROR",
      inner("via-jump", "localhost", "ProxyJump tandem-jump"),
      inner("via-command", "127.0.0.1", `ProxyCommand ssh -F ${path} -W %h:%p tandem-jump`),
      "",
    ].join("\n"),
  );
}

async function hostId(alias: string): Promise<string> {
  const hosts = await invoke<Array<{ id: string; sshTarget: string | null }>>("ssh_hosts_list");
  return hosts.find((h) => h.sshTarget === alias)?.id ?? "";
}

async function removeOurs(): Promise<void> {
  for (const alias of ALIASES) {
    const id = await hostId(alias);
    if (id) await invoke("ssh_host_remove", { hostId: id });
  }
}

describe("E2E-M14-9 hosts behind a jump host", () => {
  before(async function () {
    this.timeout(120_000);
    writeConfig();
    await freshOffline();
    await removeOurs();
  });

  after(async () => {
    await removeOurs();
  });

  for (const [alias, how] of [
    ["via-jump", "ProxyJump"],
    ["via-command", "ProxyCommand"],
  ] as const) {
    it(`F-RM-01 a host behind ${how} shows the key ssh saw through it, then connects`, async function () {
      this.timeout(180_000);
      // Settings › Remote covers the rail once open.
      if (!(await $('[data-testid="remote-hosts"]').isExisting())) await openRemotePage();
      // Just the alias, as the Add host sheet's hint says; saving connects.
      await $('[data-testid="remote-add"]').click();
      await settled();
      await $('[data-testid="remote-add-host"]').setValue(alias);
      await $('[data-testid="remote-add-save"]').click();
      await browser.waitUntil(async () => $('[data-testid="remote-hostkey-fp"]').isExisting(), {
        timeout: 45_000,
        timeoutMsg: `no host key to approve: ${JSON.stringify(await hostRows())}`,
      });
      expect(await hostKeyShown()).toBe(hostFingerprint());
      await approveHostKey();
      const id = await hostId(alias);
      expect(id).not.toBe("");
      await browser.waitUntil(
        async () => (await hostRows()).find((h) => h.id === id)?.state === "connected",
        {
          timeout: 120_000,
          timeoutMsg: `not connected: ${JSON.stringify(await hostRows())}`,
        },
      );
      const home = await invoke<{ path: string; dirs: string[] }>("ssh_list_dir", {
        hostId: id,
        path: "/home/tandem",
      });
      expect(home.dirs.some((d) => d.endsWith("project"))).toBe(true);
      await invoke("ssh_host_remove", { hostId: id });
    });
  }
});
