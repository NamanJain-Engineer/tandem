import { useSyncExternalStore } from "react";
import type { RemoteSession, SessionView } from "@tandem/remote/session";

/** The session's view, re-rendering on every change. */
export function useSessionView(session: RemoteSession): SessionView {
  return useSyncExternalStore(
    (cb) => session.subscribe(cb),
    () => session.current,
  );
}
