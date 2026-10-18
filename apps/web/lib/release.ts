/** The public builds, served from this site (`public/downloads/`). The
 * Linux .deb and .rpm are built by the product repo's release script with
 * TANDEM_HOSTED=0: no hosted service, the Free plan runs locally and
 * nothing is sent to our servers. Update with each release. */
export const DOWNLOADS_PATH = "/downloads";

export const RELEASE = {
  version: "0.8.0",
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
    requirement: "macOS 14 or later, Apple Silicon or Intel",
    assets: [{ file: "Vibezeo_0.8.0_universal-unsigned.dmg", label: "Download for macOS (.dmg)" }],
    steps: [
      "Open the .dmg and drag Vibezeo into Applications.",
      "The first time, right-click Vibezeo in Applications, choose Open, then Open again. The build isn't notarized by Apple yet, so a plain double-click is blocked.",
    ],
    available: false,
  },
  {
    os: "windows",
    name: "Windows",
    requirement: "Windows 10 or 11, 64-bit",
    assets: [
      { file: "Vibezeo_0.8.0_x64-setup-unsigned.exe", label: "Download for Windows (.exe)" },
      { file: "Vibezeo_0.8.0_x64_en-US-unsigned.msi", label: ".msi installer" },
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
    requirement: "x86-64 with WebKitGTK 4.1: Ubuntu 22.04+, Debian 12+ or Fedora 42+",
    assets: [
      { file: "Vibezeo_0.8.0_amd64.deb", label: "Download for Linux (.deb)" },
      { file: "Vibezeo-0.8.0-1.x86_64.rpm", label: "Fedora (.rpm)" },
    ],
    steps: [
      "Debian or Ubuntu: sudo apt install ./Vibezeo_0.8.0_amd64.deb",
      "Fedora: sudo dnf install ./Vibezeo-0.8.0-1.x86_64.rpm",
      "Open it from your applications menu.",
    ],
    available: true,
  },
];

export const assetUrl = (file: string): string => `${DOWNLOADS_PATH}/${file}`;
/** What's in each release: the changelog on this site. */
export const RELEASE_URL = "/changelog/";
export const CHECKSUMS_URL = assetUrl("SHA256SUMS.txt");
