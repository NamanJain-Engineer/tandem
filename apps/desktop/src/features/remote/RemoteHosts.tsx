import { useEffect, useRef, useState } from "react";
import type { HostView, IpcError } from "@tandem/protocol";
import {
  Badge,
  Button,
  Callout,
  ConfirmDialog,
  Dialog,
  EmptyState,
  Input,
  ListRow,
  SegmentedControl,
  Sheet,
  Skeleton,
  Spinner,
  useToast,
} from "@tandem/ui";
import { Cloud, Folder, FolderUp, Plus } from "lucide-react";
import { t, type MessageKey } from "../../i18n";
import { useRemote, isUp } from "../../stores/remote";
import { useWorkspaces } from "../../stores/workspaces";
import { useUi } from "../../stores/ui";
import { useSession } from "../../stores/session";

/** `user@host:port` as typed. */
export function address(h: HostView): string {
  const port = h.sshPort && h.sshPort !== 22 ? `:${h.sshPort}` : "";
  return `${h.sshTarget ?? ""}${port}`;
}

const STAGE: Record<string, MessageKey> = {
  resolving: "remote.stage.connecting",
  hostkey: "remote.stage.hostkey",
  password: "remote.stage.password",
  connecting: "remote.stage.connecting",
  installing: "remote.stage.installing",
  starting: "remote.stage.starting",
};

/** What the row says about its connection. */
function useHostStatus(h: HostView): {
  text: string;
  tone: "done" | "working" | "waiting" | "error" | "idle";
} {
  const attempt = useRemote((s) => s.attempts[h.id]);
  if (h.state === "connected") return { text: t("remote.state.connected"), tone: "done" };
  if (h.state === "reconnecting") return { text: t("remote.state.reconnecting"), tone: "waiting" };
  if (attempt && attempt.stage !== "connected" && attempt.stage !== "failed")
    return { text: t(STAGE[attempt.stage] ?? "remote.stage.connecting"), tone: "working" };
  if (attempt?.stage === "failed" && attempt.error) return { text: t("remote.state.error"), tone: "error" };
  return { text: t("remote.state.disconnected"), tone: "idle" };
}

function HostRow({ h, onOpenFolder }: { h: HostView; onOpenFolder: (h: HostView) => void }) {
  const status = useHostStatus(h);
  const attempt = useRemote((s) => s.attempts[h.id]);
  const { connect, disconnect, remove, resetKey } = useRemote.getState();
  const { show } = useToast();
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [confirmKey, setConfirmKey] = useState(false);
  const busy = status.tone === "working";
  const error = attempt?.stage === "failed" ? attempt.error : null;
  const keyChanged = error?.stage === "host_key" && !!h.hostKeyFp;
  return (
    <li data-testid="remote-host" data-host={h.id} data-state={h.state}>
      <ListRow
        leading={<Cloud size={16} aria-hidden="true" className="text-fg-muted" />}
        title={h.label}
        secondary={
          <span className="flex items-center gap-2">
            <span className="font-mono">{address(h)}</span>
            <span>{t("remote.workspaces", { n: h.workspaces })}</span>
          </span>
        }
        trailing={
          <span className="flex items-center gap-1.5">
            <Badge variant={status.tone === "idle" ? "neutral" : status.tone} dot>
              <span data-testid="remote-host-state">{status.text}</span>
            </Badge>
            {busy && <Spinner size={14} label={status.text} />}
            {isUp(h.state) ? (
              <>
                <Button
                  size="sm"
                  variant="primary"
                  iconLeft={<Folder size={14} />}
                  onClick={() => onOpenFolder(h)}
                >
                  {t("remote.openFolder")}
                </Button>
                <Button size="sm" variant="ghost" onClick={() => void disconnect(h.id)}>
                  {t("remote.disconnect")}
                </Button>
              </>
            ) : (
              <Button
                size="sm"
                variant="secondary"
                disabled={busy || h.state === "reconnecting"}
                onClick={() =>
                  void connect(h.id).catch((e: IpcError) => show({ tone: "error", title: e.message }))
                }
              >
                {t("remote.connect")}
              </Button>
            )}
            <Button size="sm" variant="ghost" onClick={() => setConfirmRemove(true)}>
              {t("remote.remove")}
            </Button>
          </span>
        }
      />
      {error && (
        <Callout
          tone="error"
          className="mx-2 mb-2"
          title={keyChanged ? t("remote.keyChanged", { host: h.label }) : undefined}
          actions={
            keyChanged ? (
              <Button size="sm" variant="danger" onClick={() => setConfirmKey(true)}>
                {t("remote.trustNewKey")}
              </Button>
            ) : undefined
          }
        >
          <span data-testid="remote-host-error" data-stage={error.stage ?? ""}>
            {error.message}
          </span>
          {keyChanged && h.hostKeyFp && (
            <span className="mt-1 block font-mono text-xs">{t("remote.pinnedKey", { fp: h.hostKeyFp })}</span>
          )}
        </Callout>
      )}
      <ConfirmDialog
        open={confirmRemove}
        onOpenChange={setConfirmRemove}
        title={t("remote.remove.title", { host: h.label })}
        body={t("remote.remove.body")}
        confirmLabel={t("remote.remove")}
        danger
        onConfirm={() => void remove(h.id).then((e) => e && show({ tone: "error", title: e.message }))}
      />
      <ConfirmDialog
        open={confirmKey}
        onOpenChange={setConfirmKey}
        title={t("remote.trustNewKey.title", { host: h.label })}
        body={t("remote.trustNewKey.body")}
        confirmLabel={t("remote.trustNewKey.confirm")}
        danger
        onConfirm={() => void resetKey(h.id).then(() => connect(h.id))}
      />
    </li>
  );
}

/** UI-14's "+ Add host" sheet. */
function AddHost({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const [label, setLabel] = useState("");
  const [target, setTarget] = useState("");
  const [user, setUser] = useState("");
  const [port, setPort] = useState("");
  const [auth, setAuth] = useState("agent");
  const [key, setKey] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  useEffect(() => {
    if (open) {
      setLabel("");
      setTarget("");
      setUser("");
      setPort("");
      setAuth("agent");
      setKey("");
      setError(null);
    }
  }, [open]);
  const portNumber = port.trim() === "" ? null : Number(port);
  const portBad =
    portNumber !== null && !(Number.isInteger(portNumber) && portNumber > 0 && portNumber < 65536);
  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const hostId = await useRemote.getState().add({
        target: target.trim(),
        user: user.trim() || null,
        port: portNumber,
        identityFile: auth === "key" ? key.trim() || null : null,
        label: label.trim() || null,
      });
      onOpenChange(false);
      void useRemote.getState().connect(hostId);
    } catch (e) {
      setError((e as IpcError).message ?? t("remote.add.failed"));
    } finally {
      setSaving(false);
    }
  };
  return (
    <Sheet
      open={open}
      onOpenChange={onOpenChange}
      title={t("remote.add.title")}
      subtitle={t("remote.add.subtitle")}
      dirty={target !== "" || label !== ""}
      footer={
        <>
          <Button variant="ghost" onClick={() => onOpenChange(false)}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            data-testid="remote-add-save"
            loading={saving}
            disabled={target.trim() === "" || portBad || (auth === "key" && key.trim() === "")}
            onClick={() => void save()}
          >
            {t("remote.add.saveConnect")}
          </Button>
        </>
      }
    >
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          void save();
        }}
      >
        <Input
          label={t("remote.add.name")}
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          placeholder={t("remote.add.namePlaceholder")}
        />
        <Input
          label={t("remote.add.host")}
          data-testid="remote-add-host"
          value={target}
          onChange={(e) => setTarget(e.target.value)}
          hint={t("remote.add.hostHint")}
          autoFocus
          spellCheck={false}
        />
        <div className="flex gap-3">
          <Input
            className="flex-1"
            label={t("remote.add.user")}
            data-testid="remote-add-user"
            value={user}
            onChange={(e) => setUser(e.target.value)}
            spellCheck={false}
          />
          <Input
            className="w-28"
            label={t("remote.add.port")}
            data-testid="remote-add-port"
            value={port}
            inputMode="numeric"
            placeholder="22"
            error={portBad ? t("remote.add.portBad") : undefined}
            onChange={(e) => setPort(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <span className="text-sm font-medium text-fg">{t("remote.add.auth")}</span>
          <SegmentedControl
            label={t("remote.add.auth")}
            value={auth}
            onValueChange={setAuth}
            items={[
              { id: "agent", label: t("remote.add.authAgent") },
              { id: "key", label: t("remote.add.authKey") },
            ]}
          />
        </div>
        {auth === "key" && (
          <Input
            label={t("remote.add.keyFile")}
            data-testid="remote-add-key"
            value={key}
            onChange={(e) => setKey(e.target.value)}
            hint={t("remote.add.keyHint")}
            spellCheck={false}
            className="font-mono"
          />
        )}
        <p className="text-sm text-fg-muted">{t("remote.add.never")}</p>
        {error && (
          <Callout tone="error">
            <span data-testid="remote-add-error">{error}</span>
          </Callout>
        )}
      </form>
    </Sheet>
  );
}

/** The remote folder picker: folders on the host, from home. */
export function RemoteFolderPicker({ host, onClose }: { host: HostView | null; onClose: () => void }) {
  const [path, setPath] = useState("");
  const [typed, setTyped] = useState("");
  const [dirs, setDirs] = useState<string[] | null>(null);
  const [parent, setParent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const { show } = useToast();
  // Only the latest listing counts: on a host whose helper was just
  // installed, the first listing (home) can come back after the folder the
  // user typed, and mustn't replace it.
  const latest = useRef(0);
  const go = async (to: string) => {
    if (!host) return;
    const n = ++latest.current;
    setDirs(null);
    setError(null);
    try {
      const l = await useRemote.getState().listDir(host.id, to);
      if (n !== latest.current) return;
      setPath(l.path);
      setTyped(l.path);
      setParent(l.parent);
      setDirs(l.dirs);
    } catch (e) {
      if (n !== latest.current) return;
      setError((e as IpcError).message);
      setDirs([]);
    }
  };
  useEffect(() => {
    if (host) void go("");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [host?.id]);
  const open = async () => {
    if (!host) return;
    setAdding(true);
    const err = await useWorkspaces.getState().addRemote({ kind: "ssh", root: path, hostId: host.id });
    setAdding(false);
    if (err) {
      if (err.code === "limit_reached")
        useUi.getState().openUpgrade({
          kind: "workspaces",
          limit: useSession.getState().entitlement?.limits.workspaces ?? 3,
        });
      else show({ tone: "error", title: err.message });
      return;
    }
    onClose();
    useUi.getState().closeSettings();
    useUi.getState().setMode("code");
  };
  return (
    <Dialog
      open={host !== null}
      onOpenChange={(o) => !o && onClose()}
      title={t("remote.picker.title", { host: host?.label ?? "" })}
      size="lg"
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button
            variant="primary"
            data-testid="remote-picker-open"
            disabled={!path || adding}
            loading={adding}
            onClick={() => void open()}
          >
            {t("remote.picker.open")}
          </Button>
        </>
      }
    >
      <div
        className="flex flex-col gap-3"
        data-testid="remote-picker"
        data-path={path}
        data-state={dirs === null ? "loading" : "ready"}
      >
        <form
          className="flex items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void go(typed);
          }}
        >
          <Input
            className="flex-1 font-mono"
            label={t("remote.picker.path")}
            data-testid="remote-picker-path"
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            spellCheck={false}
          />
          <Button type="submit" variant="secondary">
            {t("remote.picker.go")}
          </Button>
        </form>
        {error && <Callout tone="error">{error}</Callout>}
        <ul
          className="max-h-72 overflow-y-auto rounded-sm border border-border"
          aria-label={t("remote.picker.folders")}
        >
          {parent && (
            <li>
              <ListRow
                role="button"
                tabIndex={0}
                leading={<FolderUp size={14} aria-hidden="true" />}
                title={t("remote.picker.up")}
                onClick={() => void go(parent)}
                onKeyDown={(e) => e.key === "Enter" && void go(parent)}
              />
            </li>
          )}
          {dirs === null ? (
            <li className="flex flex-col gap-2 p-2">
              <Skeleton className="h-6" />
              <Skeleton className="h-6" />
            </li>
          ) : dirs.length === 0 ? (
            <li className="p-3 text-sm text-fg-muted">{t("remote.picker.empty")}</li>
          ) : (
            dirs.map((d) => (
              <li key={d} data-testid="remote-picker-dir" data-name={d}>
                <ListRow
                  role="button"
                  tabIndex={0}
                  leading={<Folder size={14} aria-hidden="true" />}
                  title={d}
                  onClick={() => void go(`${path === "/" ? "" : path}/${d}`)}
                  onKeyDown={(e) => e.key === "Enter" && void go(`${path === "/" ? "" : path}/${d}`)}
                />
              </li>
            ))
          )}
        </ul>
      </div>
    </Dialog>
  );
}

/** UI-14 / UI-111: SSH hosts, their connections, and their folders. */
export function RemoteHosts() {
  // WSL distros have their own section; this one is SSH hosts.
  const all = useRemote((s) => s.hosts);
  const hosts = all.filter((h) => h.kind === "ssh");
  const loaded = useRemote((s) => s.loaded);
  const error = useRemote((s) => s.error);
  const [adding, setAdding] = useState(false);
  const [picking, setPicking] = useState<HostView | null>(null);
  useEffect(() => {
    void useRemote.getState().refresh();
  }, []);
  return (
    <section className="flex flex-col gap-3" data-testid="remote-hosts">
      <div className="flex items-center justify-between">
        <h3 className="text-md font-semibold text-fg">{t("remote.ssh.title")}</h3>
        <Button
          size="sm"
          variant="secondary"
          iconLeft={<Plus size={14} />}
          data-testid="remote-add"
          onClick={() => setAdding(true)}
        >
          {t("remote.addHost")}
        </Button>
      </div>
      {!loaded ? (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-10" />
          <Skeleton className="h-10" />
        </div>
      ) : error ? (
        <Callout tone="error">{error}</Callout>
      ) : hosts.length === 0 ? (
        <EmptyState
          icon={<Cloud size={20} />}
          title={t("remote.empty.title")}
          body={t("remote.empty.body")}
          primary={
            <Button variant="primary" onClick={() => setAdding(true)}>
              {t("remote.addHost")}
            </Button>
          }
        />
      ) : (
        <ul className="flex flex-col gap-1">
          {hosts.map((h) => (
            <HostRow key={h.id} h={h} onOpenFolder={setPicking} />
          ))}
        </ul>
      )}
      <AddHost open={adding} onOpenChange={setAdding} />
      <RemoteFolderPicker host={picking} onClose={() => setPicking(null)} />
    </section>
  );
}
