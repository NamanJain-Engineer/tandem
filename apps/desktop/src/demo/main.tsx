import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "../App";
import "../styles.css";
import { useLayout } from "../stores/layout";
import { useUi } from "../stores/ui";
import { useWorkspaces } from "../stores/workspaces";
import { DEMO_ACTIVE, DEMO_WORKTREES, installDemo } from "./backend";
import { DemoBar } from "./DemoBar";
import { DemoCoach } from "./DemoCoach";
import { holdFocusUntilEngaged } from "./embedFocus";

// vibezeo.com/demo: the real interface against a simulated core. On the home
// page it runs in a same-origin frame (`?embed=1`, D-128).
const core = installDemo();
const embedded = new URLSearchParams(window.location.search).has("embed");
// Code mode opens on the working agent's worktree (D-1899): a state any
// visitor reaches by opening it. Each worktree's tab is loaded, as after
// launching its agent in this session, so the rail shows how every agent
// is doing from the first frame (F-SH-03).
useWorkspaces.setState({ activeId: DEMO_ACTIVE });
for (const id of DEMO_WORKTREES) void useLayout.getState().load(id).catch(() => undefined);
// Small screens start with the rail collapsed, so every surface fits.
if (window.innerWidth < 1024) useUi.setState({ railCollapsed: true });
// Esc that nothing in the app used closes the home page's full-screen view.
// And the composer's autofocus mustn't scroll the page to the frame or take
// its keyboard: nothing takes focus until the visitor clicks in (D-1821).
if (embedded && window.parent !== window) {
  holdFocusUntilEngaged(window);
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !e.defaultPrevented)
      window.parent.postMessage({ type: "vibezeo-demo:escape" }, window.location.origin);
  });
}
const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <div className="flex h-full flex-col">
        <DemoBar core={core} firstThread={core.firstThread} embedded={embedded} />
        <div className="relative min-h-0 flex-1">
          <App />
          <DemoCoach core={core} />
        </div>
      </div>
    </StrictMode>,
  );
}
