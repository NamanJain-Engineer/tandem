import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { WidgetRoot } from "./features/dictate/WidgetRoot";
import "./styles.css";

// The Dictate widget is its own window on the same page (F-DI-07).
const widget = new URLSearchParams(window.location.search).get("window") === "dictate-widget";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(<StrictMode>{widget ? <WidgetRoot /> : <App />}</StrictMode>);
}
