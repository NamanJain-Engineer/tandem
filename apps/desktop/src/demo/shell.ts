/** A tiny line-editing shell for the browser demo's terminals: it echoes
 * keys, handles Backspace, Ctrl+C and Ctrl+L, and answers a few commands
 * with canned output. Nothing runs. */

const ESC = "\x1b";
const GREEN = `${ESC}[32m`;
const BLUE = `${ESC}[34m`;
const DIM = `${ESC}[2m`;
const BOLD = `${ESC}[1m`;
const RESET = `${ESC}[0m`;

export const PROMPT = `${GREEN}you@vibezeo${RESET}:${BLUE}~/shop${RESET}$ `;

const FILES = ["README.md", "package.json", "src/", "tests/", "tsconfig.json", "vite.config.ts"];

const TEST_RUN = [
  "",
  "> shop@1.4.0 test",
  "> vitest run",
  "",
  ` ${GREEN}✓${RESET} src/checkout/redirect.test.ts (4 tests) 12ms`,
  ` ${GREEN}✓${RESET} src/checkout/cart.test.ts (9 tests) 31ms`,
  ` ${GREEN}✓${RESET} src/api/rate-limit.test.ts (6 tests) 18ms`,
  "",
  ` Test Files  ${GREEN}3 passed${RESET} (3)`,
  `      Tests  ${GREEN}19 passed${RESET} (19)`,
  "",
];

/** Canned answers; `null` clears the screen. */
export function run(line: string): string[] | null {
  const [cmd = "", ...args] = line.trim().split(/\s+/);
  switch (cmd) {
    case "":
      return [];
    case "help":
      return [
        `${BOLD}This terminal is part of a browser demo.${RESET} It understands:`,
        "  ls, pwd, git status, git log, npm test, echo, whoami, date, clear",
        `${DIM}In Vibezeo every pane is a real terminal on your machine.${RESET}`,
      ];
    case "ls":
      return [FILES.map((f) => (f.endsWith("/") ? `${BLUE}${f}${RESET}` : f)).join("  ")];
    case "pwd":
      return ["/home/you/shop"];
    case "whoami":
      return ["you"];
    case "date":
      return [new Date().toString()];
    case "echo":
      return [args.join(" ")];
    case "clear":
      return null;
    case "git":
      if (args[0] === "status")
        return [
          "On branch main",
          "Changes not staged for commit:",
          `  ${ESC}[31mmodified:   src/checkout/redirect.ts${RESET}`,
          `  ${ESC}[31mmodified:   src/checkout/redirect.test.ts${RESET}`,
        ];
      if (args[0] === "log")
        return [
          `${ESC}[33m4f2c9e1${RESET} Add cart totals endpoint`,
          `${ESC}[33m91b07aa${RESET} Checkout: keep the coupon on reload`,
          `${ESC}[33m0d5e3b2${RESET} First commit`,
        ];
      return [`git: '${args[0] ?? ""}' isn't available in this demo. Try git status or git log.`];
    case "npm":
    case "pnpm":
      if (args[0] === "test" || args[0] === "t") return TEST_RUN;
      return [`${cmd} ${args.join(" ")}: only \`${cmd} test\` works in this demo.`];
    case "claude":
    case "codex":
    case "gemini":
      return [
        `${cmd} would start here in Vibezeo. This demo has no real agents:`,
        "the panes beside this one show what they look like.",
      ];
    default:
      return [`${cmd}: command not found (type ${BOLD}help${RESET})`];
  }
}

/** One terminal's line editor. `write` gets what xterm sends and returns
 * what the "PTY" echoes back. */
export class DemoShell {
  private line = "";

  greeting(): string {
    return `${DIM}Simulated shell, type ${RESET}${BOLD}help${RESET}${DIM} to see what it knows.${RESET}\r\n${PROMPT}`;
  }

  write(data: string): string {
    let out = "";
    for (const ch of data) {
      if (ch === "\r") {
        const result = run(this.line);
        this.line = "";
        if (result === null) out += `${ESC}[2J${ESC}[H${PROMPT}`;
        else out += `\r\n${result.map((l) => `${l}\r\n`).join("")}${PROMPT}`;
      } else if (ch === "\x7f" || ch === "\b") {
        if (this.line.length > 0) {
          this.line = this.line.slice(0, -1);
          out += "\b \b";
        }
      } else if (ch === "\x03") {
        this.line = "";
        out += `^C\r\n${PROMPT}`;
      } else if (ch === "\x0c") {
        out += `${ESC}[2J${ESC}[H${PROMPT}${this.line}`;
      } else if (ch === ESC) {
        // Arrow keys and other sequences: ignored.
        break;
      } else if (ch >= " ") {
        this.line += ch;
        out += ch;
      }
    }
    return out;
  }
}

/** What an agent pane shows: a still frame of an agent's terminal UI. */
export function agentScreen(engine: string, task: string, lines: string[]): string {
  const bar = "─".repeat(58);
  return [
    `${DIM}╭${bar}╮${RESET}`,
    `${DIM}│${RESET} ${BOLD}${engine}${RESET} ${DIM}· simulated in this demo${RESET}${" ".repeat(Math.max(0, 57 - engine.length - 25))}${DIM}│${RESET}`,
    `${DIM}╰${bar}╯${RESET}`,
    "",
    `${BOLD}>${RESET} ${task}`,
    "",
    ...lines,
    "",
  ].join("\r\n");
}
