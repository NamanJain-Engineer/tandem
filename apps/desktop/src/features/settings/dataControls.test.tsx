import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import type { AuthState } from "@tandem/protocol";
import { UiProvider } from "@tandem/ui";
import { setInvokeBackend } from "../../ipc/invoke";
import { useSession } from "../../stores/session";
import { fakeBackend, resetStores, SIGNED_IN } from "../../test/fakeBackend";
import { DataControls } from "./DataControls";

const EARLY_ACCESS: AuthState = {
  signedIn: false,
  email: null,
  offline: true,
  hosted: false,
  pending: null,
  error: null,
};

const mount = () =>
  render(
    <UiProvider>
      <DataControls />
    </UiProvider>,
  );

beforeEach(resetStores);
afterEach(() => setInvokeBackend(null));

async function openDelete() {
  fireEvent.click(screen.getByRole("button", { name: "Delete local data…" }));
  return screen.findByRole("alertdialog");
}

describe("Settings › Privacy › Your data (F-TL-05, UI-112)", () => {
  it("F-TL-05.1 Export my data saves the zip where the user picked and says where", async () => {
    const calls = fakeBackend({ data_export: () => ({ path: "/home/ana/vibezeo-export.zip", tables: 46, files: 52 }) });
    mount();
    fireEvent.click(screen.getByRole("button", { name: "Export my data…" }));
    expect(await screen.findByText("Exported your data")).toBeTruthy();
    expect(screen.getByText("/home/ana/vibezeo-export.zip")).toBeTruthy();
    expect(calls.filter((c) => c.cmd === "data_export")).toHaveLength(1);
  });

  it("F-TL-05 a cancelled save says nothing; a failed export says so", async () => {
    let fail = false;
    fakeBackend({
      data_export: () => {
        if (fail) throw { code: "io", message: "The export couldn't be written: disk full" };
        return null;
      },
    });
    mount();
    const button = screen.getByRole("button", { name: "Export my data…" });
    fireEvent.click(button);
    await waitFor(() => expect((button as HTMLButtonElement).disabled).toBe(false));
    expect(screen.queryByText("Exported your data")).toBeNull();
    fail = true;
    fireEvent.click(button);
    expect(await screen.findByText("The export didn't finish")).toBeTruthy();
    expect(screen.getByText("The export couldn't be written: disk full")).toBeTruthy();
  });

  it("F-TL-05.2 Delete local data asks for 'delete' and keeps engine sign-ins unless ticked", async () => {
    const calls = fakeBackend({ data_delete_local: () => null });
    mount();
    const dialog = await openDelete();
    expect(dialog.textContent).toContain("Delete all Vibezeo data on this device?");
    expect(dialog.textContent).toContain("Your project folders and AI accounts aren't touched.");
    const box = screen.getByRole("checkbox", { name: "Also remove engine sign-ins" });
    expect(box.getAttribute("aria-checked")).toBe("false");
    const confirm = screen.getByRole("button", { name: "Delete data" });
    expect((confirm as HTMLButtonElement).disabled).toBe(true);
    fireEvent.change(screen.getByLabelText('Type "delete" to confirm'), { target: { value: "delete" } });
    fireEvent.click(confirm);
    await waitFor(() => expect(calls.some((c) => c.cmd === "data_delete_local")).toBe(true));
    expect(calls.find((c) => c.cmd === "data_delete_local")?.args).toEqual({ removeEngineSignIns: false });
  });

  it("F-TL-05.2 ticking 'Also remove engine sign-ins' says so in the body and sends it", async () => {
    const calls = fakeBackend({ data_delete_local: () => null });
    mount();
    const dialog = await openDelete();
    fireEvent.click(screen.getByRole("checkbox", { name: "Also remove engine sign-ins" }));
    expect(dialog.textContent).toContain("signs you out of every engine");
    fireEvent.change(screen.getByLabelText('Type "delete" to confirm'), { target: { value: "delete" } });
    fireEvent.click(screen.getByRole("button", { name: "Delete data" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "data_delete_local")?.args).toEqual({ removeEngineSignIns: true }),
    );
  });

  it("F-TL-05 Delete my account is offered only to a signed-in Vibezeo account, and opens the web account page", async () => {
    const calls = fakeBackend({ billing_open: () => null });
    useSession.setState({ auth: EARLY_ACCESS });
    const { unmount } = mount();
    expect(screen.queryByRole("button", { name: "Delete my account…" })).toBeNull();
    unmount();
    useSession.setState({ auth: SIGNED_IN });
    mount();
    fireEvent.click(screen.getByRole("button", { name: "Delete my account…" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "billing_open")?.args).toEqual({ target: { type: "account" } }),
    );
  });
});
