import type { Metadata } from "next";
import { ProsePage } from "../../components/site/SiteFrame";
import { OPERATOR, SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Terms", alternates: { canonical: "/terms/" } };

export default function TermsPage() {
  return (
    <ProsePage title="Terms of use">
      <p>
        These terms cover your use of this website and of the {SITE.product} desktop app, run by {OPERATOR}.
        By using either you agree to them.
      </p>

      <h2>The site</h2>
      <ul>
        <li>
          The site describes {SITE.product}, a free desktop app. Features and platforms described here may
          change.
        </li>
        <li>
          The content is provided as is, for information. We work to keep it accurate but make no warranty
          that it is complete or current.
        </li>
        <li>
          Don't misuse the site: no attempts to disrupt it, probe it without permission or scrape it at scale.
        </li>
      </ul>

      <h2>The desktop app</h2>
      <ul>
        <li>
          You may install and use the desktop app on your own computers, for personal or work projects, free
          of charge.
        </li>
        <li>Don't sell or redistribute them. Link to the download page instead.</li>
        <li>
          They're provided as is, without warranty. Software can have bugs: keep backups of your work and use
          version control.
        </li>
        <li>
          The coding agents you run in {SITE.product} use your own accounts with their providers, under those
          providers' terms.
        </li>
      </ul>

      <h2>Names and marks</h2>
      <p>
        The {SITE.product} name and logo belong to {OPERATOR}. Other product names on this site, including the
        coding agents {SITE.product} works with, belong to their owners. Their use here doesn't imply
        affiliation or endorsement.
      </p>

      <h2>Liability</h2>
      <p>
        To the extent the law allows, {OPERATOR} isn't liable for any loss that comes from using this site or
        the desktop app.
      </p>

      <h2>Contact</h2>
      <p>
        Questions about these terms go to{" "}
        <a href={`mailto:${SITE.contactEmail}`} className="text-accent hover:underline">
          {SITE.contactEmail}
        </a>
        .
      </p>
      {SITE.legal && (
        <p>
          {SITE.legal.name}
          {SITE.legal.registration && `, ${SITE.legal.registration}`}
          {SITE.legal.address && `, ${SITE.legal.address}`}.
        </p>
      )}
    </ProsePage>
  );
}
