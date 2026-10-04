import { useEffect, useState } from "react";
import type { AppBoot } from "@tandem/protocol";
import { Splash } from "./features/splash/Splash";
import { inTauri, invoke } from "./ipc/invoke";

/** M0: the window shows the UI-02 splash. The app shell arrives in M2. */
export function App() {
  const [, setBoot] = useState<AppBoot | null>(null);

  useEffect(() => {
    if (!inTauri()) return;
    // Show the window after the first paint (ARCHITECTURE §1.3 step 5).
    requestAnimationFrame(() => {
      invoke<AppBoot>("app_ready")
        .then(setBoot)
        .catch(() => undefined);
    });
  }, []);

  return <Splash />;
}
