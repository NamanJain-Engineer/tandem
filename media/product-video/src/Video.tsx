import { AbsoluteFill, Audio, Sequence, staticFile } from "remotion";
import { Backdrop, Grain, Vignette } from "./components";
import { Claude, Crash, Detect, EndCard, Hook, LogoReveal, Restore, Status, Terminals, Trust, type Beat } from "./scenes";
import { sec } from "./theme";

// Scene cuts sit on the soundtrack's bar lines (120 BPM, 2 s bars).
const CUTS = {
  hook: [0, 4],
  logo: [4, 8],
  detect: [8, 14],
  terminals: [14, 28],
  status: [28, 34],
  crash: [34, 36],
  restore: [36, 46],
  trust: [46, 52],
  claude: [52, 56],
  end: [56, 64.5],
} as const;

// Terminals: the recording runs at 2.3x from 15.0 s, so local frame = (t - 15) * 13.04.
const BEATS: Beat[] = [
  { at: 22, label: "A real shell, right in your project" },
  { at: 51, label: "Split right", keys: ["Ctrl", "Shift", "E"] },
  { at: 66, label: "Pick an agent or a terminal for each new pane" },
  { at: 100, label: "Full-screen TUIs like htop just work" },
  { at: 180, label: "Split down", keys: ["Ctrl", "Shift", "D"] },
  { at: 262, label: "Your tests and tools, unchanged" },
  { at: 352, label: "vim, git, your editor: whatever you run" },
];

// Test results land at ~34.5 s in the recording (local 254), vim opens at ~41 s (local 339).
const ZOOM: Array<[number, number, number, number]> = [
  [0, 1, 0.5, 0.5],
  [78, 1, 0.5, 0.5],
  [100, 1.9, 0.79, 0.31],
  [150, 1.9, 0.79, 0.31],
  [175, 1, 0.5, 0.5],
  [240, 1, 0.5, 0.5],
  [262, 1.9, 0.79, 0.75],
  [305, 1.9, 0.79, 0.75],
  [325, 1, 0.5, 0.5],
  [338, 1, 0.5, 0.5],
  [354, 1.9, 0.38, 0.75],
  [392, 1.9, 0.38, 0.75],
  [408, 1, 0.5, 0.5],
];

function At({ cut, children }: { cut: readonly [number, number]; children: React.ReactNode }) {
  return (
    <Sequence from={sec(cut[0])} durationInFrames={sec(cut[1]) - sec(cut[0])}>
      {children}
    </Sequence>
  );
}

export function Video() {
  return (
    <AbsoluteFill style={{ background: "#060a12" }}>
      <Backdrop />
      <At cut={CUTS.hook}>
        <Hook />
      </At>
      <At cut={CUTS.logo}>
        <LogoReveal />
      </At>
      <At cut={CUTS.detect}>
        <Detect />
      </At>
      <At cut={CUTS.terminals}>
        <Terminals beats={BEATS} zoom={ZOOM} />
      </At>
      <At cut={CUTS.status}>
        <Status />
      </At>
      <At cut={CUTS.crash}>
        <Crash />
      </At>
      <At cut={CUTS.restore}>
        <Restore
          zoom={[
            [0, 1, 0.5, 0.5],
            [300, 1.06, 0.5, 0.5],
          ]}
        />
      </At>
      <At cut={CUTS.trust}>
        <Trust />
      </At>
      <At cut={CUTS.claude}>
        <Claude />
      </At>
      <At cut={CUTS.end}>
        <EndCard />
      </At>
      <Vignette />
      <Grain />
      <Audio src={staticFile("soundtrack.wav")} />
    </AbsoluteFill>
  );
}
