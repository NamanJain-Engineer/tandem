"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Badge, Button, SegmentedControl, StatusDot, type Status } from "@tandem/ui";
import { Card, Notice } from "../../components/Shell";
import { PairingError, PhoneClient, type CommandResult, type ConnectionState } from "../../lib/remote/client";
import type {
  PhoneAgent,
  PhoneApproval,
  PhoneGrants,
  PhoneRoutine,
  PhoneSnapshot,
  PhoneThread,
  PhoneTranscript,
} from "../../lib/remote/messages";
import { parsePairingFragment } from "../../lib/remote/protocol";
import { IndexedDbPairingStore } from "../../lib/remote/store";

type Phase =
  | { kind: "loading" }
  | { kind: "unpaired" }
  | { kind: "pairing" }
  | { kind: "pair_failed"; message: string }
  | { kind: "revoked" }
  | { kind: "paired" };

type Tab = "agents" | "approvals" | "threads" | "routines";

const CONNECTION_LABEL: Record<ConnectionState, string> = {
  idle: "Not connected",
  connecting: "Connecting…",
  desktop_offline: "Your computer is offline",
  online: "Connected",
  revoked: "Unpaired",
  replaced: "Open in another tab",
};

const POLL_MS = 10_000;

function deviceLabel(): string {
  const ua = navigator.userAgent;
  const os = /iPhone|iPad/.test(ua) ? "iPhone" : /Android/.test(ua) ? "Android" : "Phone";
  const browser = /CriOS|Chrome/.test(ua) ? "Chrome" : /Safari/.test(ua) ? "Safari" : /Firefox/.test(ua) ? "Firefox" : "";
  return browser ? `${os} · ${browser}` : os;
}

function statusOf(a: PhoneAgent): Status {
  return a.status === "failed" ? "error" : a.status;
}

/** F-RM-03: the phone companion. Pairs from the QR link, then shows agents, approvals and threads. */
export function RemoteApp({ product }: { product: string }) {
  const client = useRef<PhoneClient | null>(null);
  const [phase, setPhase] = useState<Phase>({ kind: "loading" });
  const [connection, setConnection] = useState<ConnectionState>("idle");
  const [desktop, setDesktop] = useState("");
  const [grants, setGrants] = useState<PhoneGrants | null>(null);
  const [snapshot, setSnapshot] = useState<PhoneSnapshot | null>(null);
  const [snapshotError, setSnapshotError] = useState<string | null>(null);
  const [tab, setTab] = useState<Tab>("agents");
  const [rejected, setRejected] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const c = client.current;
    if (!c || c.connection !== "online") return;
    const r = await c.command<PhoneSnapshot>("view.snapshot");
    if (r.ok) {
      setSnapshot(r.data);
      setGrants(r.data.grants);
      setSnapshotError(null);
    } else setSnapshotError(r.message);
  }, []);

  useEffect(() => {
    if ("serviceWorker" in navigator) {
      navigator.serviceWorker.register("/remote/sw.js", { scope: "/remote" }).catch(() => undefined);
    }
    const c = new PhoneClient(
      new IndexedDbPairingStore(),
      {
        state: (s) => {
          setConnection(s);
          if (s === "revoked") setPhase({ kind: "revoked" });
        },
        welcome: (name, g) => {
          setDesktop(name);
          setGrants(g);
        },
        changed: () => void refresh(),
        rejected: (reason) => setRejected(reason),
      },
      undefined,
      deviceLabel(),
    );
    client.current = c;
    const link = parsePairingFragment(window.location.hash);
    // The pairing secret is in the fragment: take it out of the address bar and history.
    if (window.location.hash) history.replaceState(null, "", window.location.pathname);
    void (async () => {
      if (link) {
        setPhase({ kind: "pairing" });
        try {
          const p = await c.pair(link);
          setDesktop(p.desktop);
          setGrants(p.grants);
          setPhase({ kind: "paired" });
        } catch (err) {
          setPhase({
            kind: "pair_failed",
            message: err instanceof PairingError ? err.message : "Pairing didn't work. Make a new code and try again.",
          });
        }
        return;
      }
      const resumed = await c.resume().catch(() => false);
      if (resumed && c.paired) {
        setDesktop(c.paired.desktop);
        setGrants(c.paired.grants);
        setPhase({ kind: "paired" });
      } else setPhase({ kind: "unpaired" });
    })();
    return () => c.disconnect();
  }, [refresh]);

  useEffect(() => {
    if (phase.kind !== "paired" || connection !== "online") return;
    void refresh();
    const timer = setInterval(() => {
      if (document.visibilityState === "visible") void refresh();
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [phase.kind, connection, refresh]);

  async function unpair() {
    await client.current?.unpair();
    setSnapshot(null);
    setPhase({ kind: "unpaired" });
  }

  return (
    <div className="min-h-screen bg-bg text-fg">
      <header className="sticky top-0 z-10 border-b border-border bg-bg">
        <div className="mx-auto flex h-12 max-w-xl items-center gap-3 px-4">
          <svg width="18" height="18" viewBox="0 0 28 28" fill="none" aria-hidden="true">
            <circle cx="10.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
            <circle cx="17.5" cy="14" r="7" stroke="var(--tn-accent)" strokeWidth="2.2" />
          </svg>
          <span className="font-semibold">{product}</span>
          {phase.kind === "paired" && (
            <span className="ml-auto flex items-center gap-2 text-sm text-fg-muted" data-testid="remote-connection">
              <StatusDot status={connection === "online" ? "done" : "offline"} size={8} />
              {connection === "online" && desktop ? desktop : CONNECTION_LABEL[connection]}
            </span>
          )}
        </div>
      </header>
      <main className="mx-auto flex max-w-xl flex-col gap-4 px-4 py-6">
        {phase.kind === "loading" && <Notice>Loading…</Notice>}
        {phase.kind === "unpaired" && <PairHelp product={product} />}
        {phase.kind === "pairing" && <Notice>Pairing with your computer…</Notice>}
        {phase.kind === "pair_failed" && (
          <>
            <Notice tone="error">{phase.message}</Notice>
            <PairHelp product={product} />
          </>
        )}
        {phase.kind === "revoked" && (
          <>
            <Notice tone="error">This phone is no longer paired. Its keys have been deleted.</Notice>
            <PairHelp product={product} />
          </>
        )}
        {phase.kind === "paired" && (
          <>
            {rejected && (
              <Notice tone="error">{`Your computer refused a message from this phone (${rejected}).`}</Notice>
            )}
            {connection === "replaced" && (
              <Notice>This pairing is open in another tab or window. Close it there, then reload this page.</Notice>
            )}
            <SegmentedControl
              label="Remote view"
              value={tab}
              onValueChange={(v) => setTab(v as Tab)}
              items={[
                { id: "agents", label: "Agents" },
                { id: "approvals", label: `Approvals${snapshot?.approvals.length ? ` (${snapshot.approvals.length})` : ""}` },
                { id: "threads", label: "Threads" },
                { id: "routines", label: "Routines" },
              ]}
            />
            {snapshotError && <Notice tone="error">{snapshotError}</Notice>}
            {!snapshot && !snapshotError && (
              <Notice>{connection === "online" ? "Loading…" : CONNECTION_LABEL[connection]}</Notice>
            )}
            {snapshot && grants && client.current && (
              <>
                {tab === "agents" && <Agents agents={snapshot.agents} grants={grants} client={client.current} />}
                {tab === "approvals" && (
                  <Approvals
                    approvals={snapshot.approvals}
                    grants={grants}
                    armedUntil={snapshot.armedUntil}
                    client={client.current}
                    onDone={() => void refresh()}
                  />
                )}
                {tab === "threads" && <Threads threads={snapshot.threads} grants={grants} client={client.current} />}
                {tab === "routines" && (
                  <Routines routines={snapshot.routines} grants={grants} client={client.current} onDone={() => void refresh()} />
                )}
              </>
            )}
            <div className="mt-6 border-t border-border pt-4">
              <Button variant="danger" size="sm" onClick={() => void unpair()}>
                Unpair this phone
              </Button>
            </div>
          </>
        )}
      </main>
    </div>
  );
}

function PairHelp({ product }: { product: string }) {
  return (
    <Card>
      <h1 className="text-lg font-semibold">Pair this phone</h1>
      <p className="mt-2 text-sm text-fg-muted">
        {`On your computer, open ${product} › Settings › Remote and choose Pair a phone. Scan the code with this phone's camera, and this page opens paired.`}
      </p>
      <p className="mt-2 text-sm text-fg-muted">
        Everything between this phone and your computer is end-to-end encrypted. The relay in between only passes
        sealed messages along.
      </p>
    </Card>
  );
}

function Agents({ agents, grants, client }: { agents: PhoneAgent[]; grants: PhoneGrants; client: PhoneClient }) {
  if (agents.length === 0) return <Notice>No agents are running on your computer.</Notice>;
  return (
    <ul className="flex flex-col gap-3" data-testid="remote-agents">
      {agents.map((a) => (
        <li key={a.key} className="rounded-md border border-border bg-surface-2 p-4">
          <div className="flex items-center gap-2">
            <StatusDot status={statusOf(a)} />
            <span className="min-w-0 flex-1 truncate font-medium">{a.title}</span>
            {a.engine && <Badge>{a.engine}</Badge>}
          </div>
          {a.workspace && <p className="mt-1 text-sm text-fg-muted">{a.workspace}</p>}
          {a.preview && <p className="mt-2 truncate font-mono text-sm text-fg-muted">{a.preview}</p>}
          {a.paneId && grants.terminal && <TypeBox paneId={a.paneId} client={client} />}
        </li>
      ))}
    </ul>
  );
}

function TypeBox({ paneId, client }: { paneId: string; client: PhoneClient }) {
  const [text, setText] = useState("");
  const [result, setResult] = useState<CommandResult | null>(null);
  async function send() {
    const r = await client.command("pane.type", { paneId, text });
    setResult(r);
    if (r.ok) setText("");
  }
  return (
    <form
      className="mt-3 flex gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (text.trim()) void send();
      }}
    >
      <input
        aria-label="Type to this agent"
        value={text}
        onChange={(e) => setText(e.target.value)}
        className="h-8 min-w-0 flex-1 rounded-sm border border-border bg-surface-4 px-2.5 text-base text-fg outline-none focus:border-accent focus:shadow-focus"
      />
      <Button type="submit" size="sm" variant="secondary">
        Send
      </Button>
      {result && !result.ok && <Notice tone="error">{result.message}</Notice>}
    </form>
  );
}

function Approvals({
  approvals,
  grants,
  armedUntil,
  client,
  onDone,
}: {
  approvals: PhoneApproval[];
  grants: PhoneGrants;
  armedUntil: number | null;
  client: PhoneClient;
  onDone: () => void;
}) {
  const [message, setMessage] = useState<{ tone: "info" | "error"; text: string } | null>(null);
  async function decide(approvalId: string, decision: "allow" | "deny") {
    const r = await client.command<{ state: "applied" | "queued" }>("approval.decide", { approvalId, decision });
    if (!r.ok) setMessage({ tone: "error", text: r.message });
    else if (r.data.state === "queued")
      setMessage({ tone: "info", text: "Sent. It applies once you confirm it on your computer." });
    else setMessage({ tone: "info", text: decision === "allow" ? "Allowed." : "Denied." });
    onDone();
  }
  return (
    <div className="flex flex-col gap-3">
      {message && <Notice tone={message.tone}>{message.text}</Notice>}
      {!grants.approvals && (
        <Notice>Answering approvals from this phone is off. Turn it on in Settings › Remote on your computer.</Notice>
      )}
      {grants.approvals && !armedUntil && (
        <Notice>
          Allow waits for you to confirm on your computer, unless it&apos;s armed to accept approvals from this phone.
          Deny applies at once.
        </Notice>
      )}
      {approvals.length === 0 && <Notice>Nothing is waiting for you.</Notice>}
      <ul className="flex flex-col gap-3" data-testid="remote-approvals">
        {approvals.map((a) => (
          <li key={a.id} className="rounded-md border border-border bg-surface-2 p-4">
            <p className="font-medium">{a.summary}</p>
            <p className="mt-1 text-sm text-fg-muted">{a.source}</p>
            {a.detail && <pre className="mt-2 whitespace-pre-wrap font-mono text-sm text-fg-muted">{a.detail}</pre>}
            {a.queued ? (
              <div className="mt-3">
                <Badge variant="waiting">Waiting for you on your computer</Badge>
              </div>
            ) : (
              grants.approvals && (
                <div className="mt-3 flex gap-2">
                  <Button size="sm" variant="primary" onClick={() => void decide(a.id, "allow")}>
                    Allow
                  </Button>
                  <Button size="sm" variant="secondary" onClick={() => void decide(a.id, "deny")}>
                    Deny
                  </Button>
                </div>
              )
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

function Routines({
  routines,
  grants,
  client,
  onDone,
}: {
  routines: PhoneRoutine[];
  grants: PhoneGrants;
  client: PhoneClient;
  onDone: () => void;
}) {
  const [message, setMessage] = useState<{ tone: "info" | "error"; text: string } | null>(null);
  async function run(r: PhoneRoutine) {
    const res = await client.command("routine.run_now", { routineId: r.id });
    setMessage(res.ok ? { tone: "info", text: `${r.name} is running.` } : { tone: "error", text: res.message });
    onDone();
  }
  return (
    <div className="flex flex-col gap-3">
      {message && <Notice tone={message.tone}>{message.text}</Notice>}
      {!grants.routines && (
        <Notice>Running routines from this phone is off. Turn it on in Settings › Remote on your computer.</Notice>
      )}
      {routines.length === 0 && <Notice>No routines on your computer.</Notice>}
      <ul className="flex flex-col divide-y divide-border rounded-md border border-border bg-surface-2" data-testid="remote-routines">
        {routines.map((r) => (
          <li key={r.id} className="flex items-center gap-2 px-4 py-3">
            <span className="min-w-0 flex-1 truncate">{r.name}</span>
            {r.running && <Badge variant="working">Running</Badge>}
            {r.paused && <Badge>Paused</Badge>}
            {grants.routines && !r.running && (
              <Button size="sm" variant="secondary" onClick={() => void run(r)}>
                Run now
              </Button>
            )}
          </li>
        ))}
      </ul>
    </div>
  );
}

function Threads({ threads, grants, client }: { threads: PhoneThread[]; grants: PhoneGrants; client: PhoneClient }) {
  const [open, setOpen] = useState<PhoneThread | null>(null);
  if (open) return <Transcript thread={open} grants={grants} client={client} onBack={() => setOpen(null)} />;
  if (threads.length === 0) return <Notice>No threads yet.</Notice>;
  return (
    <ul className="flex flex-col divide-y divide-border rounded-md border border-border bg-surface-2">
      {threads.map((t) => (
        <li key={t.id}>
          <button type="button" className="flex w-full items-center gap-2 px-4 py-3 text-left" onClick={() => setOpen(t)}>
            <span className="min-w-0 flex-1 truncate">{t.title}</span>
            <span className="text-sm text-fg-muted">{t.state}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

function Transcript({
  thread,
  grants,
  client,
  onBack,
}: {
  thread: PhoneThread;
  grants: PhoneGrants;
  client: PhoneClient;
  onBack: () => void;
}) {
  const [page, setPage] = useState<PhoneTranscript | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [text, setText] = useState("");

  const load = useCallback(
    async (before?: number) => {
      const r = await client.command<PhoneTranscript>("thread.read", before ? { threadId: thread.id, before } : { threadId: thread.id });
      if (!r.ok) {
        setError(r.message);
        return;
      }
      setError(null);
      setPage((prev) =>
        before && prev ? { items: [...r.data.items, ...prev.items], before: r.data.before } : r.data,
      );
    },
    [client, thread.id],
  );

  useEffect(() => {
    void load();
  }, [load]);

  async function send() {
    const r = await client.command("thread.send", { threadId: thread.id, text });
    if (!r.ok) setError(r.message);
    else {
      setText("");
      void load();
    }
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-2">
        <Button size="sm" variant="ghost" onClick={onBack}>
          ‹ Threads
        </Button>
        <span className="truncate font-medium">{thread.title}</span>
      </div>
      {error && <Notice tone="error">{error}</Notice>}
      {!page && !error && <Notice>Loading…</Notice>}
      {page?.before && (
        <Button size="sm" variant="secondary" onClick={() => void load(page.before ?? undefined)}>
          Load earlier
        </Button>
      )}
      {page && page.items.length === 0 && <Notice>Nothing in this thread yet.</Notice>}
      <ol className="flex flex-col gap-2" data-testid="remote-transcript">
        {page?.items.map((m, i) => (
          <li
            key={`${m.at}-${i}`}
            className={`rounded-md px-3 py-2 text-sm ${m.role === "user" ? "self-end bg-surface-3" : "bg-surface-2"}`}
          >
            <p className="whitespace-pre-wrap">{m.text}</p>
          </li>
        ))}
      </ol>
      {grants.chat ? (
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            if (text.trim()) void send();
          }}
        >
          <input
            aria-label="Message"
            value={text}
            onChange={(e) => setText(e.target.value)}
            className="h-8 min-w-0 flex-1 rounded-sm border border-border bg-surface-4 px-2.5 text-base text-fg outline-none focus:border-accent focus:shadow-focus"
          />
          <Button type="submit" size="sm" variant="primary">
            Send
          </Button>
        </form>
      ) : (
        <Notice>Sending from this phone is off. Turn on Chat control in Settings › Remote on your computer.</Notice>
      )}
    </div>
  );
}
