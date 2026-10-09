import { browser } from "@wdio/globals";
import { execFileSync, spawnSync } from "node:child_process";
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
    ["compose", "-f", join(root, "infra/docker-compose.yml"), "--profile", "remote", "exec", "-T", "sshd", "sh", "-c", cmd],
    { encoding: "utf8" },
  );
}

/** Copies a local file into the container. */
export function copyIn(local: string, remote: string): void {
  const id = execFileSync("docker", ["compose", "-f", join(root, "infra/docker-compose.yml"), "--profile", "remote", "ps", "-q", "sshd"], {
    encoding: "utf8",
  }).trim();
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

/** `ssh -O exit` on the app's master connection for the fixture: the
 * connection and every session on it drop (E2E-M14-4). */
export function dropConnection(): void {
  spawnSync("ssh", [
    "-o",
    `ControlPath=${join(home(), "run", "ssh-%C")}`,
    "-p",
    "2222",
    "-l",
    "tandem",
    "-O",
    "exit",
    "127.0.0.1",
  ]);
}

/** Hosts as the Remote page lists them. */
export async function hostRows(): Promise<Array<{ id: string; state: string; text: string; error: string; stage: string }>> {
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
  return String(await browser.execute(() => document.querySelector('[data-testid="remote-hostkey-fp"]')?.textContent ?? ""));
}

export async function waitForState(state: string, timeout = 60_000): Promise<void> {
  await until(async () => (await hostRows()).some((h) => h.state === state), `no host is ${state}: ${JSON.stringify(await hostRows())}`, timeout);
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
  await until(
    async () => (await browser.execute(() => (document.querySelector('[data-testid="remote-picker-path"]') as HTMLInputElement | null)?.value ?? "")) === path,
    `the picker didn't go to ${path}`,
  );
  await $('[data-testid="remote-picker-open"]').click();
  await until(async () => (await workspaceRoots()).includes(path), `${path} wasn't added: ${JSON.stringify(await workspaceRoots())}`);
  await until(async () => (await paneIds()).length >= 1, "the remote shell didn't open");
  return (await paneIds())[0] ?? "";
}

/** The fixture's host id, once it's been added. */
export async function fixtureHostId(): Promise<string> {
  const hosts = await invoke<Array<{ id: string; sshTarget: string | null; sshPort: number | null }>>("ssh_hosts_list");
  return hosts.find((h) => h.sshTarget === "tandem@127.0.0.1" && h.sshPort === 2222)?.id ?? "";
}
