import { describe, expect, it } from "vitest";
import { DEMO_DESKTOP, DemoDesktop } from "./demo";
import { b64url, encodePairingLink, generateKeyPair } from "./protocol";
import { linkFromUrl, noticesBetween, relayAllowed, RemoteSession, type Notice } from "./session";
import { MemoryPairingStore } from "./store";

const LINK = {
  v: 1 as const,
  pid: "pr_sessiontest01",
  dpk: b64url(generateKeyPair().public),
  s: b64url(new Uint8Array(32).fill(1)),
  relay: "ws://192.168.1.20:47800/v1/relay/pr_sessiontest01",
  ptok: "tok",
};

async function until(check: () => boolean, ms = 2000): Promise<void> {
  const end = Date.now() + ms;
  while (!check()) {
    if (Date.now() > end) throw new Error("timed out");
    await new Promise((r) => setTimeout(r, 5));
  }
}

async function demoSession(notices: Notice[] = []) {
  const desktop = new DemoDesktop();
  const session = new RemoteSession(new MemoryPairingStore(), {
    socketFactory: desktop.socketFactory(),
    device: "Test phone",
    demo: true,
    onNotice: (n) => notices.push(n),
  });
  await session.start();
  expect(session.current.phase.kind).toBe("unpaired");
  expect(await session.pair(desktop.link())).toBe(true);
  await until(() => session.current.snapshot !== null);
  return { desktop, session };
}

describe("F-RM-05 pairing links", () => {
  it("F-RM-05.1 reads the code from the desktop's web link, a LAN link and the app's own link", () => {
    for (const base of [
      "https://app.vibezeo.com/remote",
      "http://192.168.1.20:47800/remote",
      "vibezeo-remote://pair",
    ]) {
      expect(linkFromUrl(encodePairingLink(base, LINK))).toEqual(LINK);
    }
    const query = encodePairingLink("vibezeo-remote://pair", LINK).replace("#p=", "?p=");
    expect(linkFromUrl(query)).toEqual(LINK);
    expect(linkFromUrl("https://example.com/")).toBeNull();
    expect(linkFromUrl("not a url#p=%%%")).toBeNull();
  });

  it("F-RM-04.6 connects to wss:// anywhere but ws:// only on a private network or VPN", () => {
    expect(relayAllowed("wss://api.vibezeo.com/v1/relay/pr_x")).toBe(true);
    for (const ok of ["ws://192.168.1.20:47800/v1/relay/pr_x", "ws://10.0.0.2:1/x", "ws://172.20.1.1:1/x", "ws://100.100.1.1:1/x", "ws://127.0.0.1:1/x"]) {
      expect(relayAllowed(ok), ok).toBe(true);
    }
    for (const bad of ["ws://8.8.8.8:1/x", "ws://example.com:1/x", "ws://172.32.0.1:1/x", "ws://100.128.0.1:1/x", "http://192.168.1.2/x", "demo://x", "nonsense"]) {
      expect(relayAllowed(bad), bad).toBe(false);
    }
  });

  it("F-RM-04.6 a code that points at a plain-text server on the internet isn't followed", async () => {
    const session = new RemoteSession(new MemoryPairingStore(), {
      socketFactory: () => {
        throw new Error("must not connect");
      },
    });
    await session.start();
    expect(await session.pair({ ...LINK, relay: "ws://203.0.113.9:47800/v1/relay/pr_x" })).toBe(false);
    expect(session.current.phase).toMatchObject({ kind: "pair_failed" });
  });
});

describe("F-RM-05 demo mode", () => {
  it("F-RM-05.2 pairs with the demo computer over the real protocol and shows its snapshot", async () => {
    const { session } = await demoSession();
    expect(session.current.phase.kind).toBe("paired");
    expect(session.current.connection).toBe("online");
    expect(session.current.desktop).toBe(DEMO_DESKTOP);
    const snap = session.current.snapshot;
    expect(snap?.agents.map((a) => a.status)).toEqual(["working", "waiting", "done"]);
    expect(snap?.threads.some((t) => t.teammate)).toBe(true);
    session.stop();
  });

  it("F-RM-05.2 deny applies at once; allow waits for the computer (F-TL-06 rule 5)", async () => {
    const { session } = await demoSession();
    const approval = session.current.snapshot?.approvals[0];
    if (!approval) throw new Error("no approval");
    const allowed = await session.decide(approval, "allow");
    expect(allowed).toEqual({ ok: true, data: { state: "queued" } });
    await until(() => session.current.snapshot?.approvals[0]?.queued === true);
    const denied = await session.decide(approval, "deny");
    expect(denied).toEqual({ ok: true, data: { state: "applied" } });
    await until(() => session.current.snapshot?.approvals.length === 0);
    session.stop();
  });

  it("F-RM-05.2 reads and sends in threads and teammate chats, types to agents and runs routines", async () => {
    const { desktop, session } = await demoSession();
    const page = await session.readThread("th_demo2");
    expect(page.ok && page.data.items.length).toBe(2);
    expect((await session.sendToThread("th_demo2", "Thanks!")).ok).toBe(true);
    const after = await session.readThread("th_demo2");
    expect(after.ok && after.data.items.some((i) => i.text === "Thanks!")).toBe(true);
    expect((await session.typeToPane("pn_demo1", "run the tests again")).ok).toBe(true);
    expect(desktop.typed).toEqual([{ paneId: "pn_demo1", text: "run the tests again" }]);
    expect((await session.runRoutine("rt_demo1")).ok).toBe(true);
    const paused = await session.runRoutine("rt_demo2");
    expect(paused).toMatchObject({ ok: false, code: "paused" });
    session.stop();
  });

  it("F-RM-05.3 a finished agent and a new approval become notices", async () => {
    const notices: Notice[] = [];
    const { desktop, session } = await demoSession(notices);
    desktop.advance();
    await until(() => notices.length >= 1);
    expect(notices[0]).toMatchObject({ kind: "agent", title: "Checkout tests finished" });
    desktop.advance();
    await until(() => notices.length >= 2);
    expect(notices[1]).toMatchObject({ kind: "approval", body: "Edit `src/checkout/cart.ts`" });
    session.stop();
  });

  it("F-RM-05.4 unpairing forgets the computer", async () => {
    const { session } = await demoSession();
    await session.unpair();
    expect(session.current.phase.kind).toBe("unpaired");
    expect(session.current.snapshot).toBeNull();
  });

  it("F-RM-05.2 demo mode reaches only the demo computer", async () => {
    const desktop = new DemoDesktop();
    const session = new RemoteSession(new MemoryPairingStore(), { socketFactory: desktop.socketFactory(), demo: true });
    await session.start();
    expect(await session.pair(LINK)).toBe(false);
  });
});

describe("F-RM-05.3 notices", () => {
  it("none on the first snapshot; queued allows don't notify", () => {
    const desktop = new DemoDesktop();
    const snap = desktop.snapshot();
    expect(noticesBetween(null, snap)).toEqual([]);
    const next = { ...snap, approvals: [...snap.approvals, { ...snap.approvals[0]!, id: "ap_x", queued: true }] };
    expect(noticesBetween(snap, next)).toEqual([]);
  });
});
