import { Composition } from "remotion";
import { INTER, MONO_FONT } from "./fonts";
import { Video } from "./Video";
import { DURATION, FPS, H, W } from "./theme";

function bytes(b64: string): ArrayBuffer {
  const s = atob(b64);
  const out = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i);
  return out.buffer;
}

// Embedded fonts: the renderer waits for document.fonts.ready before each frame.
if (typeof document !== "undefined") {
  document.fonts.add(new FontFace("Inter", bytes(INTER), { weight: "100 900" }));
  document.fonts.add(new FontFace("JetBrains Mono", bytes(MONO_FONT), { weight: "100 800" }));
}

export function Root() {
  return <Composition id="Product" component={Video} durationInFrames={DURATION} fps={FPS} width={W} height={H} />;
}
