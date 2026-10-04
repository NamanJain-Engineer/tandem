"use client";

import { useCallback, useEffect, useState } from "react";
import { Button } from "@tandem/ui";
import { Card, Notice, Shell } from "../../components/Shell";
import { api, ApiFailure, formatDate } from "../../lib/api";

type Device = {
  id: string;
  deviceName: string | null;
  platform: string | null;
  appVersion: string | null;
  lastUsedAt: string;
};

export default function DevicesPage() {
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setDevices(await api<Device[]>("/v1/me/sessions"));
    } catch (err) {
      if (err instanceof ApiFailure && err.status === 401) {
        window.location.replace("/signin?next=/devices");
        return;
      }
      setError("Couldn't load your devices.");
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function revoke(id: string) {
    await api(`/v1/me/sessions/${encodeURIComponent(id)}`, { method: "DELETE" });
    await load();
  }

  return (
    <Shell title="Devices" subtitle="Desktop apps signed in to your account.">
      {error && <Notice tone="error">{error}</Notice>}
      {!devices && !error && <Notice>Loading…</Notice>}
      {devices && devices.length === 0 && <Notice>No desktop app is signed in.</Notice>}
      {devices && devices.length > 0 && (
        <Card>
          <ul className="divide-y divide-divider">
            {devices.map((d) => (
              <li key={d.id} className="flex items-center justify-between py-3">
                <div>
                  <p className="text-base">{d.deviceName ?? "Desktop"}</p>
                  <p className="text-sm text-fg-muted">
                    {[d.platform, d.appVersion && `v${d.appVersion}`, `last used ${formatDate(d.lastUsedAt)}`]
                      .filter(Boolean)
                      .join(" · ")}
                  </p>
                </div>
                <Button variant="secondary" size="sm" onClick={() => revoke(d.id)}>
                  Revoke
                </Button>
              </li>
            ))}
          </ul>
        </Card>
      )}
    </Shell>
  );
}
