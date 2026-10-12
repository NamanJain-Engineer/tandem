import { browser } from "@wdio/globals";
import { execFileSync, spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { home, invoke, paneIds, settled, workspaceRoots } from "./app";

/** Remote workspaces (E2E-M14): the `sshd` compose service on :2222
 * (infra/docker-compose.yml, profile `remote`), user `tandem`, key
 * infra/ssh/id_ed25519, a repository at /home/tandem/project. */
export const root = new URL("../../../..", import.meta.url).pathname;
export const KEY = join(root, "infra/ssh/id_ed25519");
export const PROJECT = "/home/tandem/project";

async function until(check: () => Promise<boolean>, timeoutMsg: string, timeout = 30_000): Promise<void> {
  await browser.waitUntil(check, { timeout, timeoutMsg });
}

/** Runs `cmd` in the sshd container as root. */
export function inContainer(cmd: string): string {
  return execFileSync(
    "docker",
    [
      "compose",
      "-f",
      join(root, "infra/docker-compose.yml"),
      "--profile",
      "remote",
      "exec",
      "-T",
      "sshd",
      "sh",
      "-c",
      cmd,
    ],
    { encoding: "utf8" },
  );
}

/** The fixture's repository as the image made it (specs edit it). */
/** The fixture's project as `infra/ssh/setup.sh` made it: its one commit
 * on `main`, nothing else committed, no other branch or remote, no
 * untracked file (a spec that commits or pushes, E2E-M14-7, leaves none
 * of it for the next). */
export function resetProject(): void {
  inContainer(
    `su tandem -c 'cd ${PROJECT} && git checkout -q -f main && git reset -q --hard "$(git rev-list --max-parents=0 HEAD)" && git clean -fdxq && rm -rf .tandem && git worktree prune && for b in $(git for-each-ref --format="%(refname:short)" refs/heads); do [ "$b" = main ] || git branch -q -D "$b"; done && for r in $(git remote); do git remote remove "$r"; done'`,
  );
}

/** Copies a local file into the container. */
export function copyIn(local: string, remote: string): void {
  const id = execFileSync(
    "docker",
    ["compose", "-f", join(root, "infra/docker-compose.yml"), "--profile", "remote", "ps", "-q", "sshd"],
    {
      encoding: "utf8",
    },
  ).trim();
  execFileSync("docker", ["cp", "-L", local, `${id}:${remote}`]);
}

/** The fixture's current ed25519 host key fingerprint, as ssh-keygen prints it. */
export function hostFingerprint(): string {
  return inContainer("ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub").split(" ")[1] ?? "";
}

/** A new host key, as after the machine was reinstalled (E2E-M14-2). */
export function regenerateHostKeys(): void {
  inContainer("rm -f /etc/ssh/ssh_host_* && ssh-keygen -A >/dev/null && pkill -HUP -x sshd || true");
}

/** `ssh -O exit` on the app's master connections (one per host entry,
 * `run/ssh-<entry>-%C`): the connection and every session on it drop
 * (E2E-M14-4). */
export function dropConnection(): void {
  const run = join(home(), "run");
  for (const name of readdirSync(run).filter((n) => /^ssh-[0-9a-f]{8}-[0-9a-f]+$/.test(n))) {
    spawnSync("ssh", ["-o", `ControlPath=${join(run, name)}`, "-O", "exit", "fixture"]);
  }
}

/** Hosts as the Remote page lists them. */
export async function hostRows(): Promise<
  Array<{ id: string; state: string; text: string; error: string; stage: string }>
> {
  return (await browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-testid="remote-host"]')).map((el) => ({
      id: el.dataset.host ?? "",
      state: el.dataset.state ?? "",
      text: el.querySelector('[data-testid="remote-host-state"]')?.textContent ?? "",
      error: el.querySelector('[data-testid="remote-host-error"]')?.textContent ?? "",
      stage: el.querySelector<HTMLElement>('[data-testid="remote-host-error"]')?.dataset.stage ?? "",
    })),
  )) as Array<{ id: string; state: string; text: string; error: string; stage: string }>;
}

/** Opens Settings › Remote from the rail's "Remote hosts". */
export async function openRemotePage(): Promise<void> {
  await $('[data-testid="rail-remote-hosts"]').click();
  await $('[data-testid="remote-hosts"]').waitForExist({ timeout: 10_000 });
}

/** Adds the fixture through "Add host" (key file auth). Leaves the
 * connection attempt running. */
export async function addFixtureHost(): Promise<void> {
  await openRemotePage();
  await $('[data-testid="remote-add"]').click();
  await settled();
  await $('[data-testid="remote-add-host"]').setValue("127.0.0.1");
  await $('[data-testid="remote-add-user"]').setValue("tandem");
  await $('[data-testid="remote-add-port"]').setValue("2222");
  await $("button=Key file…").click();
  await $('[data-testid="remote-add-key"]').setValue(KEY);
  await $('[data-testid="remote-add-save"]').click();
}

/** Waits for the host-key dialog and returns the fingerprint it shows. */
export async function hostKeyShown(): Promise<string> {
  const fp = $('[data-testid="remote-hostkey-fp"]');
  await fp.waitForExist({ timeout: 30_000 });
  // A click while the dialog zooms in can miss (D-52).
  await settled();
  return String(
    await browser.execute(
      () => document.querySelector('[data-testid="remote-hostkey-fp"]')?.textContent ?? "",
    ),
  );
}

export async function waitForState(state: string, timeout = 60_000): Promise<void> {
  await until(
    async () => (await hostRows()).some((h) => h.state === state),
    `no host is ${state}: ${JSON.stringify(await hostRows())}`,
    timeout,
  );
}

/** Opens `path` on the connected fixture through the folder picker and
 * waits for its workspace and first shell. Returns the pane id. */
export async function openRemoteFolder(path = PROJECT): Promise<string> {
  await $("button=Open folder…").click();
  await $('[data-testid="remote-picker-dir"]').waitForExist({ timeout: 30_000 });
  const field = $('[data-testid="remote-picker-path"]');
  await field.clearValue();
  await field.setValue(path);
  await browser.keys("Enter");
  // The listing of `path`, not just the typed text: "Open this folder" opens
  // the folder last listed, and is off until a listing arrives. The first
  // listing after the helper's upload can take a while.
  await until(
    async () =>
      (await browser.execute(() => {
        const p = document.querySelector('[data-testid="remote-picker"]');
        return p?.getAttribute("data-state") === "ready" ? (p.getAttribute("data-path") ?? "") : "";
      })) === path,
    `the picker didn't list ${path}`,
    60_000,
  );
  await $('[data-testid="remote-picker-open"]').waitForEnabled({ timeout: 10_000 });
  // A click while the dialog animates can miss (D-52).
  await settled();
  await $('[data-testid="remote-picker-open"]').click();
  await until(
    async () => (await workspaceRoots()).includes(path),
    `${path} wasn't added: ${JSON.stringify(await workspaceRoots())}`,
    60_000,
  );
  await until(async () => (await paneIds()).length >= 1, "the remote shell didn't open");
  return (await paneIds())[0] ?? "";
}

/** The fixture's host id, once it's been added. */
export async function fixtureHostId(): Promise<string> {
  const hosts =
    await invoke<Array<{ id: string; sshTarget: string | null; sshPort: number | null }>>("ssh_hosts_list");
  return hosts.find((h) => h.sshTarget === "tandem@127.0.0.1" && h.sshPort === 2222)?.id ?? "";
}

/** The fixture host added and connected, whatever earlier specs did:
 * added through the command when missing, its fingerprint approved when
 * asked. Leaves Settings › Remote open. */
export async function ensureConnected(): Promise<string> {
  if (!(await fixtureHostId())) {
    await invoke("ssh_host_add", {
      target: "tandem@127.0.0.1",
      port: 2222,
      identityFile: KEY,
      label: null,
      user: null,
    });
  }
  await openRemotePage();
  await until(async () => (await hostRows()).length === 1, "no fixture host");
  if ((await hostRows())[0]?.state !== "connected") {
    await $("button=Connect").click();
    await until(
      async () =>
        (await hostRows())[0]?.state === "connected" ||
        (await $('[data-testid="remote-hostkey-accept"]').isExisting()),
      "the fixture didn't connect or ask about its key",
      120_000,
    );
    if (await $('[data-testid="remote-hostkey-accept"]').isExisting()) {
      expect(await hostKeyShown()).toBe(hostFingerprint());
      await approveHostKey();
    }
    await waitForState("connected", 120_000);
  }
  return fixtureHostId();
}

/** Clicks "Trust and connect" until the dialog has gone (a click on a
 * dialog that just opened can be dropped under load, D-52). */
export async function approveHostKey(): Promise<void> {
  await settled();
  await $('[data-testid="remote-hostkey-accept"]').click();
  const gone = await browser
    .waitUntil(async () => !(await $('[data-testid="remote-hostkey"]').isExisting()), { timeout: 3_000 })
    .catch(() => false);
  if (!gone && (await $('[data-testid="remote-hostkey-accept"]').isExisting()))
    await $('[data-testid="remote-hostkey-accept"]').click();
}
