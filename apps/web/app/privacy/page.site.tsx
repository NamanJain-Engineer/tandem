import type { Metadata } from "next";
import hosted from "@tandem/protocol/hosted-features.json";
import subprocessors from "@tandem/protocol/subprocessors.json";
import { ArrowRight } from "lucide-react";
import { Icon } from "../../components/site/Icon";
import { ProsePage } from "../../components/site/SiteFrame";
import { RELEASE } from "../../lib/release";
import { OPERATOR, SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Privacy", alternates: { canonical: "/privacy/" } };

type HostedFeature = {
  id: string;
  label: string;
  sends: string;
  destination: string;
  state: { kind: string };
};
type Subprocessor = { id: string; name: string; purpose: string; data: string };

/** When a row's data is sent, in words (as /transparency says it). */
function when(f: HostedFeature): string {
  switch (f.state.kind) {
    case "signed_in":
      return "While you're signed in";
    case "on_use":
      return "Only when you use it";
    case "policy":
      return "Only when your Team admins turn it on";
    default:
      return f.id === "updates" ? "On unless you turn it off" : "Off unless you turn it on";
  }
}

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
          The current release ({RELEASE.version}) has no {SITE.product} accounts and doesn't contact our servers
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

      <h2>What the app can send, and when</h2>
      <p>
        The download on this site runs with {SITE.product}&apos;s hosted services turned off, so it sends none
        of this. When {SITE.product} accounts open, these are the only things the app sends anywhere. Some of
        them belong to features that aren&apos;t in the app yet; this is what they will send. The app shows
        the same list, with what&apos;s on, in Settings › Privacy.
      </p>
      <table className="stack-table w-full text-left text-copy-sm">
        <thead>
          <tr>
            <th className="py-2.5 pr-4">Feature</th>
            <th className="py-2.5 pr-4">What it sends</th>
            <th className="py-2.5 pr-4">To</th>
            <th className="py-2.5">When</th>
          </tr>
        </thead>
        <tbody>
          {(hosted.features as HostedFeature[]).map((f) => (
            <tr key={f.id} data-hosted-feature={f.id} className="border-t border-line align-top">
              <td data-label="Feature" className="py-2.5 pr-4 font-medium text-fg">{f.label}</td>
              <td data-label="What it sends" className="py-2.5 pr-4">{f.sends}</td>
              <td data-label="To" className="py-2.5 pr-4">
                {f.destination.split(" → ").flatMap((part, i) =>
                  i === 0
                    ? [part]
                    : [
                        <Icon key={i} icon={ArrowRight} size={14} className="mx-1 inline align-[-2px]" />,
                        <span key={`t${i}`} className="sr-only">
                          {" to "}
                        </span>,
                        part,
                      ],
                )}
              </td>
              <td data-label="When" className="py-2.5">{when(f)}</td>
            </tr>
          ))}
        </tbody>
      </table>

      <h2>Who processes it</h2>
      <ul>
        {(subprocessors.subprocessors as Subprocessor[]).map((s) => (
          <li key={s.id}>
            <strong>{s.name}</strong>: {s.purpose}. {s.data}.
          </li>
        ))}
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
