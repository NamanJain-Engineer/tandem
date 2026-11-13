"use client";

import { useEffect } from "react";
import "./light.css";

/** The site's pointer light (D-2576): an element marked `data-lit` gets a soft
 * light that follows the pointer across it, and its edge brightens nearest
 * the pointer (`light.css`). One listener for the page, at most one update a
 * frame, only for a fine pointer and never with reduced motion. Without it
 * (no JavaScript, touch, reduced motion) the surfaces simply stay unlit. */
export function PointerLight() {
  useEffect(() => {
    const fine = window.matchMedia("(hover: hover) and (pointer: fine)");
    const still = window.matchMedia("(prefers-reduced-motion: reduce)");
    let frame = 0;
    let last: { el: HTMLElement; x: number; y: number } | null = null;
    const paint = () => {
      frame = 0;
      if (!last) return;
      const r = last.el.getBoundingClientRect();
      last.el.style.setProperty("--mx", `${last.x - r.left}px`);
      last.el.style.setProperty("--my", `${last.y - r.top}px`);
    };
    const move = (e: PointerEvent) => {
      if (!fine.matches || still.matches) return;
      const el = (e.target as Element | null)?.closest?.("[data-lit]");
      if (!(el instanceof HTMLElement)) return;
      last = { el, x: e.clientX, y: e.clientY };
      if (!frame) frame = requestAnimationFrame(paint);
    };
    document.addEventListener("pointermove", move, { passive: true });
    return () => {
      document.removeEventListener("pointermove", move);
      if (frame) cancelAnimationFrame(frame);
    };
  }, []);
  return null;
}
