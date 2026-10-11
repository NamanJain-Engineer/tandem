import { useCallback, useEffect, useState } from "react";
import type {
  IpcError,
  LanState,
  Pairing,
  PairingQr,
  PhoneArmState,
  PhoneGrants,
  QueuedPhoneAction,
} from "@tandem/protocol";
import {
  Badge,
  Button,
  Callout,
  ConfirmDialog,
  Dialog,
  EmptyState,
  QRCode,
  Select,
  SettingsGroup,
  SettingsRow,
  Skeleton,
  Switch,
  useToast,
} from "@tandem/ui";
import { t } from "../../i18n";
import { copyText } from "../../ipc/clipboard";
import { listen } from "../../ipc/events";
import { invoke } from "../../ipc/invoke";
import { useSession } from "../../stores/session";
import { ago } from "../notifications/NotificationsPopover";

type RemoteEvent = { pairingId: string | null; event: string };

const GRANTS: {
  key: keyof PhoneGrants;
  label: "remote.grant.chat" | "remote.grant.terminal" | "remote.grant.approvals" | "remote.grant.routines";
}[] = [
  { key: "chat", label: "remote.grant.chat" },
  { key: "terminal", label: "remote.grant.terminal" },
  { key: "approvals", label: "remote.grant.approvals" },
  { key: "routines", label: "remote.grant.routines" },
];

function message(err: unknown): string {
  return (err as IpcError | undefined)?.message ?? String(err);
}

/** UI-111 Settings › Remote, the phone part (F-RM-03). */
export function RemotePage() {
  const auth = useSession((s) => s.auth);
  const entitlement = useSession((s) => s.entitlement);
  const [pairings, setPairings] = useState<Pairing[] | null>(null);
  const [queue, setQueue] = useState<QueuedPhoneAction[]>([]);
  const [arm, setArm] = useState<PhoneArmState>({ armedUntil: null });
  const [error, setError] = useState<string | null>(null);
  const [pairing, setPairing] = useState<PairVia | null>(null);
  const [lan, setLan] = useState<LanState | null>(null);
  const [lanAddress, setLanAddress] = useState<string | null>(null);
  const [revoking, setRevoking] = useState<Pairing | null>(null);
  const [resetting, setResetting] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const { show } = useToast();

  const load = useCallback(async () => {
    try {
      const [list, items, armed] = await Promise.all([
        invoke<Pairing[]>("phone_pairings_list"),
        invoke<QueuedPhoneAction[]>("phone_queue_list"),
        invoke<PhoneArmState>("phone_arm_state"),
      ]);
      setPairings(list);
      setQueue(items);
      setLan(await invoke<LanState>("phone_lan_state").catch(() => null));
      setArm(armed);
      setError(null);
      setNow(Date.now());
    } catch (err) {
      setError(message(err));
    }
  }, []);

  useEffect(() => {
    void load();
    let off: (() => void) | undefined;
    void listen<RemoteEvent>("remote:phone", () => void load()).then((u) => (off = u));
    return () => off?.();
  }, [load]);

  const policyOff = entitlement?.policy?.disablePhone ?? false;
  const proOff = !(entitlement?.limits as { phone?: boolean } | undefined)?.phone;
  const signedOut = !auth?.signedIn;
  // A build without Vibezeo accounts (TANDEM_HOSTED=0) has no sign-in, so it
  // never asks for one (D-833).
  const noAccounts = !!auth && !auth.hosted;
  const blocked = policyOff
    ? t("policy.setByOrg")
    : noAccounts
      ? t("remote.earlyAccess")
      : signedOut
        ? t("remote.needsSignIn")
        : proOff
          ? t("remote.needsPro")
          : null;

  async function setLanOn(on: boolean) {
    try {
      setLan(await invoke<LanState>("phone_lan_set", { enabled: on }));
    } catch (err) {
      show({ title: message(err), tone: "error" });
    }
  }

  async function setGrant(p: Pairing, key: keyof PhoneGrants, on: boolean) {
    const grants = { ...p.grants, [key]: on };
    setPairings((list) => list?.map((x) => (x.id === p.id ? { ...x, grants } : x)) ?? null);
    try {
      await invoke("phone_pairing_set_grants", { pairingId: p.id, grants });
    } catch (err) {
      show({ title: message(err), tone: "error" });
      void load();
    }
  }

  async function setArmed(on: boolean) {
    try {
      setArm(await invoke<PhoneArmState>("phone_arm_approvals", { hours: on ? 8 : 0 }));
    } catch (err) {
      show({ title: message(err), tone: "error" });
    }
  }

  async function decide(item: QueuedPhoneAction, apply: boolean) {
    try {
      await invoke("phone_queue_decide", { itemId: item.id, apply });
    } catch (err) {
      show({ title: message(err), tone: "error" });
    }
    void load();
  }

  return (
    <section className="flex flex-col gap-4" data-testid="remote-phone">
      <div className="flex flex-col gap-1">
        <h2 className="text-md font-semibold text-fg">{t("remote.phone.title")}</h2>
        <p className="text-sm text-fg-muted">{t("remote.phone.body")}</p>
      </div>
      {blocked && <Callout>{blocked}</Callout>}
      <div>
        <Button
          variant="primary"
          disabled={!!blocked}
          onClick={() => setPairing({ kind: "relay" })}
          data-testid="remote-pair"
        >
          {t("remote.pair")}
        </Button>
      </div>

      <LanSection
        lan={lan}
        policyOff={policyOff}
        address={lanAddress}
        onAddress={setLanAddress}
        onToggle={(on) => void setLanOn(on)}
        onPair={(address) => setPairing({ kind: "lan", address })}
      />

      {queue.length > 0 && (
        <SettingsGroup label={t("remote.queue")}>
          {queue.map((item) => (
            <div key={item.id} className="flex items-center gap-3 px-4 py-3" data-testid="remote-queue-item">
              <span className="min-w-0 flex-1 text-base text-fg">
                {t("remote.queue.item", { device: item.deviceName, summary: item.summary })}
              </span>
              <Button size="sm" variant="primary" onClick={() => void decide(item, true)}>
                {t("remote.queue.allow")}
              </Button>
              <Button size="sm" variant="secondary" onClick={() => void decide(item, false)}>
                {t("remote.queue.drop")}
              </Button>
            </div>
          ))}
        </SettingsGroup>
      )}

      <SettingsGroup label={t("remote.devices")}>
        {error && (
          <div className="flex items-center gap-3 px-4 py-3">
            <span className="flex-1 text-sm text-danger-text">{t("remote.devices.error")}</span>
            <Button size="sm" variant="secondary" onClick={() => void load()}>
              {t("remote.retry")}
            </Button>
          </div>
        )}
        {!error && pairings === null && (
          <div className="flex flex-col gap-2 px-4 py-3" aria-label={t("remote.loading")}>
            <Skeleton className="h-5 w-48" />
            <Skeleton className="h-5 w-64" />
          </div>
        )}
        {!error && pairings?.length === 0 && (
          <EmptyState title={t("remote.devices.empty")} body={t("remote.devices.emptyBody")} />
        )}
        {pairings?.map((p) => (
          <div key={p.id} className="flex flex-col gap-2 px-4 py-3" data-testid="remote-device">
            <div className="flex items-center gap-2">
              <span className="font-medium text-fg">{p.deviceName}</span>
              {p.online && <Badge variant="done">{t("remote.online")}</Badge>}
              <span className="ml-auto text-sm text-fg-subtle">
                {p.lastSeen ? t("remote.lastSeen", { ago: ago(p.lastSeen, now) }) : null}
              </span>
              <Button size="sm" variant="danger" onClick={() => setRevoking(p)} data-testid="remote-revoke">
                {t("remote.revoke")}
              </Button>
            </div>
            <div className="flex flex-col gap-1">
              {GRANTS.map((g) => (
                <span key={g.key} data-testid={`remote-grant-${g.key}`}>
                  <Switch
                    label={t(g.label)}
                    checked={p.grants[g.key]}
                    onCheckedChange={(on) => void setGrant(p, g.key, on)}
                  />
                </span>
              ))}
            </div>
          </div>
        ))}
      </SettingsGroup>

      {!!pairings?.length && (
        <SettingsGroup>
          <SettingsRow
            settingKey="phone_arm_approvals"
            label={t("remote.arm")}
            description={
              arm.armedUntil
                ? t("remote.arm.until", {
                    time: new Date(arm.armedUntil).toLocaleTimeString([], {
                      hour: "2-digit",
                      minute: "2-digit",
                    }),
                  })
                : t("remote.arm.desc")
            }
            control={(ids) => (
              <Switch
                id={ids.id}
                aria-describedby={ids.describedBy}
                aria-label={t("remote.arm")}
                checked={!!arm.armedUntil}
                onCheckedChange={(on) => void setArmed(on)}
              />
            )}
          />
        </SettingsGroup>
      )}

      <div>
        <Button variant="danger" onClick={() => setResetting(true)} disabled={!pairings?.length}>
          {t("remote.reset")}
        </Button>
      </div>

      {pairing && <PairDialog via={pairing} onClose={() => setPairing(null)} />}
      <ConfirmDialog
        open={revoking !== null}
        onOpenChange={(o) => !o && setRevoking(null)}
        title={t("remote.revoke.title", { device: revoking?.deviceName ?? "" })}
        body={t("remote.revoke.body")}
        confirmLabel={t("remote.revoke")}
        danger
        onConfirm={() => {
          const p = revoking;
          if (p)
            void invoke("phone_pairing_revoke", { pairingId: p.id })
              .catch((err) => show({ title: message(err), tone: "error" }))
              .finally(() => void load());
        }}
      />
      <ConfirmDialog
        open={resetting}
        onOpenChange={setResetting}
        title={t("remote.reset.title")}
        body={t("remote.reset.body")}
        confirmLabel={t("remote.reset.confirm")}
        danger
        onConfirm={() =>
          void invoke("phone_reset")
            .catch((err) => show({ title: message(err), tone: "error" }))
            .finally(() => void load())
        }
      />
    </section>
  );
}

type PairVia = { kind: "relay" } | { kind: "lan"; address: string | null };

/** F-RM-04: pairing over the local network, with the desktop as the relay. */
function LanSection({
  lan,
  policyOff,
  address,
  onAddress,
  onToggle,
  onPair,
}: {
  lan: LanState | null;
  policyOff: boolean;
  address: string | null;
  onAddress: (ip: string) => void;
  onToggle: (on: boolean) => void;
  onPair: (address: string | null) => void;
}) {
  const chosen = lan?.addresses.find((a) => a.ip === address)?.ip ?? lan?.addresses[0]?.ip ?? null;
  return (
    <SettingsGroup label={t("remote.lan.title")}>
      <div data-testid="remote-lan">
        <SettingsRow
          settingKey="phone_lan"
          label={t("remote.lan.toggle")}
          description={t("remote.lan.desc")}
          control={(ids) => (
            <Switch
              id={ids.id}
              aria-describedby={ids.describedBy}
              aria-label={t("remote.lan.toggle")}
              checked={!!lan?.enabled}
              disabled={lan === null || policyOff}
              onCheckedChange={onToggle}
            />
          )}
        />
        {lan === null && (
          <div className="px-4 py-3" aria-label={t("remote.lan.loading")}>
            <Skeleton className="h-5 w-56" />
          </div>
        )}
        {lan?.enabled && (
          <div className="flex flex-col gap-3 px-4 py-3">
            {lan.error && (
              <div data-testid="remote-lan-error">
                <Callout tone="error">{lan.error}</Callout>
              </div>
            )}
            {lan.running && (
              <>
                <span className="text-sm text-fg-muted" data-testid="remote-lan-listening">
                  {t("remote.lan.listening", { addresses: lan.addresses.map((a) => `${a.ip}:${lan.port}`).join(", ") })}
                </span>
                {lan.addresses.length > 1 && chosen && (
                  <Select
                    label={t("remote.lan.address")}
                    value={chosen}
                    onValueChange={onAddress}
                    options={lan.addresses.map((a) => ({
                      value: a.ip,
                      label: t(a.kind === "vpn" ? "remote.lan.kind.vpn" : "remote.lan.kind.lan", { ip: a.ip }),
                    }))}
                  />
                )}
                <div>
                  <Button variant="primary" onClick={() => onPair(chosen)} data-testid="remote-pair-lan">
                    {t("remote.lan.pair")}
                  </Button>
                </div>
              </>
            )}
            <span className="text-sm text-fg-subtle">{t("remote.lan.away")}</span>
          </div>
        )}
      </div>
    </SettingsGroup>
  );
}

function PairDialog({ via, onClose }: { via: PairVia; onClose: () => void }) {
  const [qr, setQr] = useState<PairingQr | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [paired, setPaired] = useState<string | null>(null);
  const { show } = useToast();

  useEffect(() => {
    let live = true;
    let id: string | null = null;
    let off: (() => void) | undefined;
    const args = via.kind === "lan" ? { via: "lan", address: via.address } : undefined;
    invoke<PairingQr>("phone_pair_begin", args, { timeoutMs: 30_000 })
      .then((code) => {
        if (!live) {
          void invoke("phone_pair_cancel", { pairingId: code.pairingId }).catch(() => undefined);
          return;
        }
        id = code.pairingId;
        setQr(code);
      })
      .catch((err) => live && setError(message(err)));
    void listen<RemoteEvent>("remote:phone", async (ev) => {
      if (!id || ev.pairingId !== id || (ev.event !== "connected" && ev.event !== "paired")) return;
      const list = await invoke<Pairing[]>("phone_pairings_list").catch(() => []);
      const found = list.find((p) => p.id === id);
      if (found && live) setPaired(found.deviceName);
    }).then((u) => (off = u));
    return () => {
      live = false;
      off?.();
      // A code nobody scanned is dropped when the dialog closes.
      if (id) void invoke("phone_pair_cancel", { pairingId: id }).catch(() => undefined);
    };
    // The dialog pairs once, the way it was opened.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      title={t("remote.dialog.title")}
      description={t(via.kind === "lan" ? "remote.dialog.bodyLan" : "remote.dialog.body")}
      footer={
        <Button variant={paired ? "primary" : "secondary"} onClick={onClose}>
          {paired ? t("remote.dialog.done") : t("remote.dialog.cancel")}
        </Button>
      }
    >
      <div className="flex flex-col items-center gap-3" data-testid="remote-pair-dialog">
        {error && <Callout tone="error">{error}</Callout>}
        {!error && !qr && (
          <div className="flex flex-col items-center gap-2">
            <Skeleton className="h-[224px] w-[224px]" />
            <span className="text-sm text-fg-muted">{t("remote.dialog.making")}</span>
          </div>
        )}
        {qr && !paired && (
          <>
            <QRCode rows={qr.qr.rows} label={t("remote.dialog.qr")} />
            <span className="text-sm text-fg-muted">{t("remote.dialog.waiting")}</span>
            <div className="flex w-full flex-col gap-1">
              <span className="text-sm text-fg-muted">{t("remote.dialog.link")}</span>
              <div className="flex items-center gap-2">
                <code
                  className="min-w-0 flex-1 truncate rounded-sm bg-surface-3 px-2 py-1 font-mono text-sm text-fg"
                  data-testid="remote-pair-link"
                >
                  {qr.link}
                </code>
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() =>
                    void copyText(qr.link).then((ok) => ok && show({ title: t("remote.dialog.copied") }))
                  }
                >
                  {t("remote.dialog.copy")}
                </Button>
              </div>
            </div>
          </>
        )}
        {paired && (
          <p className="text-base text-fg" data-testid="remote-paired">
            {t("remote.dialog.paired", { device: paired })}
          </p>
        )}
      </div>
    </Dialog>
  );
}
