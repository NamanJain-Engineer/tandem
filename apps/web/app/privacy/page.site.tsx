import type { Metadata } from "next";
import { ProsePage } from "../../components/site/SiteFrame";
import { RELEASE } from "../../lib/release";
import { OPERATOR, SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Privacy", alternates: { canonical: "/privacy/" } };

export default function PrivacyPage() {
  return (
    <ProsePage title="Privacy">
      <p>
        {OPERATOR} is responsible for your data. This page covers this website and the {SITE.product} desktop
        app. Questions go to{" "}
        <a href={`mailto:${SITE.contactEmail}`} className="text-accent hover:underline">
          {SITE.contactEmail}
        </a>
        .
      </p>

      <h2>This website</h2>
      <ul>
        <li>
          The site is a set of static pages. It sets no cookies and loads no analytics, ads or tracking
          scripts.
        </li>
        <li>
          There are no forms or accounts here. If you email us, we use your message and address only to reply.
        </li>
        <li>
          Our host, {SITE.host}, processes the technical data needed to deliver pages (such as IP address and
          browser type) under its own privacy policy.
        </li>
      </ul>

      <h2>The desktop app</h2>
      <ul>
        <li>
          The current release ({RELEASE.version}) has no {SITE.product} accounts and don't contact our servers
          at all: no sign-in, no analytics, no crash reports, no update checks.
        </li>
        <li>
          Your workspaces, layouts, settings and history are stored on your computer, in the app's data
          folder.
        </li>
        <li>
          Secrets are stored only in your operating system's keychain. Logs are scrubbed of tokens and keys.
        </li>
        <li>
          Usage statistics and crash reports are off by default. Nothing is sent unless you turn them on in
          Settings › Privacy.
        </li>
        <li>
          Coding agents (engine CLIs) sign in by themselves, in their own folders on your disk, and talk to
          their vendors directly under those vendors' terms. {SITE.product} never reads, copies or forwards
          their credentials, and doesn't send your prompts, code or terminal output to us.
        </li>
        <li>
          Once accounts open: if you sign in to a {SITE.product} account, our service stores your email
          address, your plan, and the devices you've signed in on (device name, platform and app version), so
          you can see and revoke them.
        </li>
        <li>
          Once automatic updates arrive, the app will send its version, platform and update channel to check
          for them. Updates are verified with a signature before they install.
        </li>
      </ul>

      <h2>Your choices</h2>
      <ul>
        <li>You can turn usage statistics and crash reports on or off at any time.</li>
        <li>You can ask us to delete your {SITE.product} account and its data by emailing us.</li>
        <li>We don't sell personal data, and we don't use your code or prompts to train models.</li>
      </ul>

      <h2>Changes</h2>
      <p>
        We'll update this page before anything here changes, and before accounts or automatic updates go live.
        The date at the top shows the latest version.
      </p>
    </ProsePage>
  );
}
