// The TANDEM_OPEN_URL_CMD target for desktop E2E (MILESTONES M2): instead of
// a real browser, drive headless Chromium through what the app opened.
//   /desktop-login → sign in on the web, follow the redirect to the app's
//                    loopback callback ("You can return to Vibezeo.")
//   /billing/mock  → sign in, complete the mock checkout
// Credentials come from $TANDEM_HOME/e2e-browser.json, written by the spec.
// Every step is appended to $TANDEM_HOME/e2e-browser.log.
import { appendFileSync, existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { chromium, type Page } from "playwright-core";

const home = process.env.TANDEM_HOME ?? ".";
const logFile = join(home, "e2e-browser.log");
const say = (line: string) => appendFileSync(logFile, `${new Date().toISOString()} ${line}\n`);

type Creds = { email: string; password: string; checkout?: "complete" | "cancel" };

/** A plugin's sign-in at the mock authorization server (M12): the mock
 * approves at once and redirects to the app's callback, which this follows.
 * With `plugin-signin.txt` saying "hold", the URL is only written down
 * (`plugin-authorize.txt`) for the spec to follow later (a late callback). */
async function pluginSignIn(url: URL) {
  writeFileSync(join(home, "plugin-authorize.txt"), url.toString());
  const mode = existsSync(join(home, "plugin-signin.txt"))
    ? readFileSync(join(home, "plugin-signin.txt"), "utf8").trim()
    : "follow";
  if (mode === "hold") {
    say("plugin:held");
    return;
  }
  const r = await fetch(url, { redirect: "follow" });
  say(`plugin:callback ${r.status}`);
}

async function signInThenGo(page: Page, target: URL, creds: Creds) {
  const next = target.pathname + target.search;
  await page.goto(`${target.origin}/signin?next=${encodeURIComponent(next)}`);
  await page.getByLabel("Email").fill(creds.email);
  await page.getByLabel("Password").fill(creds.password);
  await page.getByRole("button", { name: "Sign in", exact: true }).click();
}

async function main() {
  const raw = process.argv[2];
  if (!raw) throw new Error("no URL");
  const url = new URL(raw);
  if (url.port === "8791" && url.pathname === "/authorize") {
    await pluginSignIn(url);
    return;
  }
  const creds = JSON.parse(readFileSync(join(home, "e2e-browser.json"), "utf8")) as Creds;
  say(`open ${url.pathname}`);
  // A preinstalled Chromium when given (PLAYWRIGHT_CHROMIUM), else Playwright's own.
  const executablePath = process.env.PLAYWRIGHT_CHROMIUM && existsSync(process.env.PLAYWRIGHT_CHROMIUM) ? process.env.PLAYWRIGHT_CHROMIUM : undefined;
  const browser = await chromium.launch({ executablePath, headless: true });
  try {
    const page = await browser.newPage();
    if (url.pathname === "/desktop-login") {
      await signInThenGo(page, url, creds);
      await page.waitForURL(/127\.0\.0\.1:\d+\/callback/, { timeout: 30_000 });
      await page.getByText("You can return to Vibezeo.").waitFor({ timeout: 10_000 });
      say("signin:returned You can return to Vibezeo.");
    } else if (url.pathname === "/billing/mock") {
      await signInThenGo(page, url, creds);
      await page.waitForURL(/\/billing\/mock/, { timeout: 30_000 });
      if (creds.checkout === "cancel") {
        say("checkout:cancelled");
        return;
      }
      await page.getByRole("button", { name: "Complete purchase" }).click();
      await page.waitForURL(/\/billing\/success/, { timeout: 30_000 });
      say("checkout:complete");
    } else {
      await page.goto(url.toString());
      say(`visited ${url.pathname}`);
    }
  } catch (e) {
    say(`error ${(e as Error).message.split("\n")[0]}`);
    process.exitCode = 1;
  } finally {
    await browser.close();
  }
}

void main();
