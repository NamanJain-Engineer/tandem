import { create } from "zustand";
import type { SubsystemFailed } from "@tandem/protocol";
import { listen } from "../ipc/events";

/** Subsystems the watchdog gave up on (PRODUCT_SPEC §5.2): each restarted 3
 * times in 10 minutes and failed again. The shell shows a banner. */
type HealthStore = { failed: SubsystemFailed[] };

export const useHealth = create<HealthStore>(() => ({ failed: [] }));

let following = false;

export function followHealth(): void {
  if (following) return;
  following = true;
  void listen<SubsystemFailed>("app:subsystem-failed", (f) => {
    const failed = useHealth.getState().failed.filter((x) => x.name !== f.name);
    useHealth.setState({ failed: [...failed, f] });
  });
}

/** Tests. */
export function resetHealth(): void {
  following = false;
  useHealth.setState({ failed: [] });
}
