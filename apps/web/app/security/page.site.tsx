import type { Metadata } from "next";
import { ProsePage } from "../../components/site/SiteFrame";
import { SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Security", alternates: { canonical: "/security/" } };

export default function SecurityPage() {
  return (
    <ProsePage title="Security">
      <p>
        {SITE.product} runs coding agents on your machine, against your code. These are the rules it's built
        on, and how to tell us about a problem.
      </p>

      <h2>How {SITE.product} is built</h2>
      <ul>
        <li>
          <strong className="text-fg">Engine credentials stay with the engine.</strong> Each coding agent
          signs in by itself, in its own provider folder. {SITE.product} never reads, copies or proxies an AI
          vendor's tokens.
        </li>
        <li>
          <strong className="text-fg">Secrets live in the OS keychain.</strong> {SITE.product}'s own session
          tokens are stored in the macOS Keychain, Windows Credential Manager or the Secret Service on Linux,
          never in files or settings.
        </li>
        <li>
          <strong className="text-fg">Logs are redacted.</strong> Every log line passes a redactor that
          removes vendor keys, JWTs, private keys and other secrets before it's written.
        </li>
        <li>
          <strong className="text-fg">File access is contained.</strong> Paths are resolved and checked
          against the workspace, so tricks like <code className="font-mono text-sm">..</code>, symlinks and
          look-alike characters can't escape it.
        </li>
        <li>
          <strong className="text-fg">Approvals stay with you.</strong> When an agent asks permission to run a
          tool, {SITE.product} marks it as waiting for you and never answers on your behalf. Unattended work
          never inherits interactive full access.
        </li>
        <li>
          <strong className="text-fg">Updates are signed.</strong> The app verifies each update's signature
          before installing it and refuses older versions.
        </li>
        <li>
          <strong className="text-fg">Least privilege in the app.</strong> The interface runs under a strict
          content security policy with only the commands it needs. Each release ships a software bill of
          materials for its Rust and JavaScript dependencies.
        </li>
        <li>
          <strong className="text-fg">Nothing leaves without consent.</strong> Usage statistics and crash
          reports are off until you turn them on.
        </li>
      </ul>

      <h2>Reporting a vulnerability</h2>
      <p>
        Email{" "}
        <a
          href={`mailto:${SITE.contactEmail}?subject=Security%20report`}
          className="text-accent hover:underline"
        >
          {SITE.contactEmail}
        </a>{" "}
        with "Security report" in the subject. Include what you found, how to reproduce it and the version
        affected. Please give us a chance to fix it before you disclose it publicly. We'll acknowledge your
        report and keep you updated.
      </p>
    </ProsePage>
  );
}
