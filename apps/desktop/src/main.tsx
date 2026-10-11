import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { WidgetRoot } from "./features/dictate/WidgetRoot";
import { RingRoot } from "./features/conductor/FloatingRing";
import "./styles.css";

// The Dictate widget (F-DI-07) and Conductor's floating ring (F-CN-04) are
// their own windows on the same page.
const which = new URLSearchParams(window.location.search).get("window");

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      {which === "dictate-widget" ? <WidgetRoot /> : which === "conductor-ring" ? <RingRoot /> : <App />}
    </StrictMode>,
  );
}
