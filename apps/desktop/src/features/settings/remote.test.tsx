import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { Pairing, PairingQr, QueuedPhoneAction } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { emitLocal } from "../../ipc/events";
import { setInvokeBackend } from "../../ipc/invoke";
import { useSession } from "../../stores/session";
import { FREE, FREE_VIEW, fakeBackend, resetStores, type Call } from "../../test/fakeBackend";
import { RemotePage } from "./RemotePage";

const PRO_VIEW = { ...FREE_VIEW, plan: "pro" as const, limits: { ...FREE, phone: true } };

const PHONE: Pairing = {
  id: "pr_01JTESTPAIRING000000000000",
  deviceName: "iPhone · Safari",
  grants: { chat: false, terminal: false, approvals: false, routines: false },
  createdAt: 1,
  lastSeen: Date.now() - 5 * 60_000,
  online: true,
};

const QR: PairingQr = {
  pairingId: "pr_01JTESTPAIRING000000000001",
  link: "https://app.vibezeo.com/remote#p=abc",
  qr: { size: 2, rows: ["10", "01"] },
  expiresAt: Date.now() + 600_000,
};

let calls: Call[] = [];
const named = (cmd: string) => calls.filter((c) => c.cmd === cmd);

function setup(handlers: Parameters<typeof fakeBackend>[0] = {}) {
  calls = fakeBackend({
    phone_pairings_list: () => [],
    phone_queue_list: () => [],
    phone_arm_state: () => ({ armedUntil: null }),
    ...handlers,
  });
  useSession.setState({ entitlement: PRO_VIEW });
  return render(
    <UiProvider>
      <RemotePage />
    </UiProvider>,
  );
}

beforeEach(() => resetStores());
afterEach(() => setInvokeBackend(null));

describe("F-RM-03 Settings › Remote", () => {
  it("F-RM-03 empty: no phones, and Pair a phone is offered on Pro", async () => {
    setup();
    expect(await screen.findByText("No phones are paired")).toBeTruthy();
    expect((screen.getByTestId("remote-pair") as HTMLButtonElement).disabled).toBe(false);
  });

  it("F-RM-03 the Free plan and an org policy keep pairing off", async () => {
    setup();
    act(() => useSession.setState({ entitlement: FREE_VIEW }));
    expect(await screen.findByText("Pairing a phone is included with Pro.")).toBeTruthy();
    expect((screen.getByTestId("remote-pair") as HTMLButtonElement).disabled).toBe(true);
    act(() =>
      useSession.setState({
        entitlement: { ...PRO_VIEW, policy: { ...(FREE_VIEW.policy ?? {}), disablePhone: true } } as typeof PRO_VIEW,
      }),
    );
    expect(await screen.findByText("Set by your organization")).toBeTruthy();
  });

  it("F-RM-03 error: a failed load says so and retries", async () => {
    let fail = true;
    setup({
      phone_pairings_list: () => {
        if (fail) throw { code: "internal", message: "boom" };
        return [PHONE];
      },
    });
    expect(await screen.findByText("Couldn't load paired phones.")).toBeTruthy();
    fail = false;
    fireEvent.click(screen.getByText("Try again"));
    expect(await screen.findByText("iPhone · Safari")).toBeTruthy();
  });

  it("F-RM-03 populated: grants start off and toggle one at a time; revoke confirms first", async () => {
    setup({ phone_pairings_list: () => [PHONE], phone_pairing_set_grants: () => null, phone_pairing_revoke: () => null });
    expect(await screen.findByText("iPhone · Safari")).toBeTruthy();
    expect(screen.getByText("Connected")).toBeTruthy();
    const terminal = screen.getByTestId("remote-grant-terminal").querySelector("button,[role=switch]") as HTMLElement;
    expect(terminal.getAttribute("aria-checked")).toBe("false");
    fireEvent.click(terminal);
    await waitFor(() => expect(named("phone_pairing_set_grants")).toHaveLength(1));
    expect(named("phone_pairing_set_grants")[0]?.args).toEqual({
      pairingId: PHONE.id,
      grants: { ...PHONE.grants, terminal: true },
    });
    fireEvent.click(screen.getByTestId("remote-revoke"));
    expect(named("phone_pairing_revoke")).toHaveLength(0);
    await screen.findByText("Revoke iPhone · Safari?");
    // The dialog's own Revoke button is the last one.
    fireEvent.click(screen.getAllByRole("button", { name: "Revoke" }).at(-1) as HTMLElement);
    await waitFor(() => expect(named("phone_pairing_revoke")).toHaveLength(1));
  });

  it("F-RM-03.3 a phone's queued allow is confirmed here, and arming is a switch", async () => {
    const item: QueuedPhoneAction = {
      id: "pq_01JTESTQUEUE00000000000000",
      pairingId: PHONE.id,
      deviceName: "iPhone · Safari",
      approvalId: "ap_1",
      summary: "Run pnpm test",
      createdAt: 1,
    };
    setup({
      phone_pairings_list: () => [PHONE],
      phone_queue_list: () => [item],
      phone_queue_decide: () => null,
      phone_arm_approvals: () => ({ armedUntil: Date.now() + 8 * 3_600_000 }),
    });
    expect(await screen.findByText("iPhone · Safari asked to allow: Run pnpm test")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Allow" }));
    await waitFor(() => expect(named("phone_queue_decide")[0]?.args).toEqual({ itemId: item.id, apply: true }));
    fireEvent.click(screen.getByRole("switch", { name: "Allow approvals from phone for the next 8 hours" }));
    await waitFor(() => expect(named("phone_arm_approvals")[0]?.args).toEqual({ hours: 8 }));
    expect(await screen.findByText(/On until/)).toBeTruthy();
  });

  it("F-RM-03 pairing shows the code and its link, then the paired phone", async () => {
    let list: Pairing[] = [];
    setup({ phone_pair_begin: () => QR, phone_pairings_list: () => list, phone_pair_cancel: () => null });
    await screen.findByText("No phones are paired");
    fireEvent.click(screen.getByTestId("remote-pair"));
    expect(await screen.findByRole("img", { name: "Pairing code" })).toBeTruthy();
    expect(screen.getByTestId("remote-pair-link").textContent).toBe(QR.link);
    list = [{ ...PHONE, id: QR.pairingId }];
    act(() => emitLocal("remote:phone", { pairingId: QR.pairingId, event: "paired" }));
    expect(await screen.findByText("Paired with iPhone · Safari.")).toBeTruthy();
  });
});
