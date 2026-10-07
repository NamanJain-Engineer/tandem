/** The public builds: prereleases in the tandem-releases repository, built
 * by the product repo's Release workflow with channel `early-access` (no
 * hosted service: the Free plan runs locally, nothing is sent to our
 * servers). Update with each release. */
export const RELEASES_REPO = "https://github.com/NamanJain-Engineer/tandem-releases";

export const RELEASE = {
  version: "0.1.0",
  tag: "v0.1.0-early-access",
  /** When the release was published. */
  date: "October 2026",
} as const;

export type Os = "macos" | "windows" | "linux";

export type Download = {
  os: Os;
  name: string;
  requirement: string;
  assets: { file: string; label: string }[];
  /** How to install an unsigned build on this OS. */
  steps: string[];
  /** False until this OS's build is in the release: the card says "Coming soon" and links nothing. */
  available: boolean;
};

export const DOWNLOADS: Download[] = [
  {
    os: "macos",
    name: "macOS",
    requirement: "macOS 10.15 or later, Apple Silicon or Intel",
    assets: [{ file: "Tandem_0.1.0_universal-unsigned.dmg", label: "Download for macOS (.dmg)" }],
    steps: [
      "Open the .dmg and drag Tandem into Applications.",
      "The first time, right-click Tandem in Applications, choose Open, then Open again. The build isn't notarized by Apple yet, so a plain double-click is blocked.",
    ],
    available: false,
  },
  {
    os: "windows",
    name: "Windows",
    requirement: "Windows 10 or 11, 64-bit",
    assets: [
      { file: "Tandem_0.1.0_x64-setup-unsigned.exe", label: "Download for Windows (.exe)" },
      { file: "Tandem_0.1.0_x64_en-US-unsigned.msi", label: ".msi installer" },
    ],
    steps: [
      "Run the installer.",
      "If SmartScreen says it protected your PC, choose More info, then Run anyway. The build isn't code-signed yet.",
    ],
    available: false,
  },
  {
    os: "linux",
    name: "Linux",
    requirement: "x86-64 with WebKitGTK 4.1: Ubuntu 22.04+, Debian 12+, Fedora 38+",
    assets: [{ file: "Tandem_0.1.0_amd64-unsigned.deb", label: "Download for Linux (.deb)" }],
    steps: ["Debian or Ubuntu: sudo apt install ./Tandem_*.deb", "Open Tandem from your applications menu."],
    available: true,
  },
];

export const assetUrl = (file: string): string => `${RELEASES_REPO}/releases/download/${RELEASE.tag}/${file}`;
export const RELEASE_URL = `${RELEASES_REPO}/releases/tag/${RELEASE.tag}`;
export const CHECKSUMS_URL = assetUrl("SHA256SUMS");
export const ISSUES_URL = `${RELEASES_REPO}/issues`;
