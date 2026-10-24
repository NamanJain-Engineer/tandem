# Security

Please report a security problem privately, by email to **contact@vibezeo.com**, and not in a public issue or pull request. Say what you found, how to reproduce it, and which version you used (Settings › Updates shows it).

Please give Vibezeo a reasonable time to ship a fix before you publish the details.

## What's covered

- The Vibezeo desktop app and the helpers it ships: `tandem-host` (SSH workspaces), `tandem-hook`, `tandem-tasks` and the plugin gateway.
- The website, [vibezeo.com](https://vibezeo.com).

## What Vibezeo promises

- AI vendors' tokens are never read, copied or proxied: each engine signs in by itself, in its own provider home.
- Secrets are kept only in the system keychain, and no log line contains a token, prompt text or file contents.
- Nothing leaves your computer unless you turn it on.

The threat model is [`build-kit/ARCHITECTURE.md` §16](build-kit/ARCHITECTURE.md#16-security-and-privacy-model-f-tl--53), and each of its rules has a test in [`tests/invariants/`](tests/invariants/). The docs' privacy page, [vibezeo.com/docs/privacy](https://vibezeo.com/docs/privacy/), says what stays on your computer and what the app can send.
