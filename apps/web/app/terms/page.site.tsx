import type { Metadata } from "next";
import { ProsePage } from "../../components/site/SiteFrame";
import { OPERATOR, SITE } from "../../lib/site";

export const metadata: Metadata = { title: "Terms" };

export default function TermsPage() {
  return (
    <ProsePage title="Terms of use" updated={SITE.updated}>
      <p>
        These terms cover your use of this website, run by {OPERATOR}. By using the site you agree to them.
      </p>

      <h2>The site</h2>
      <ul>
        <li>
          The site describes {SITE.product}, a desktop app in active development. Features, platforms and
          plans described here may change before release.
        </li>
        <li>
          The content is provided as is, for information. We work to keep it accurate but make no warranty
          that it is complete or current.
        </li>
        <li>
          Don't misuse the site: no attempts to disrupt it, probe it without permission or scrape it at scale.
        </li>
      </ul>

      <h2>Early builds</h2>
      <p>
        If we give you a pre-release build of {SITE.product}, it comes with its own terms. Pre-release
        software can have bugs. Keep backups of your work and use version control.
      </p>

      <h2>Names and marks</h2>
      <p>
        {SITE.product} and its logo belong to {OPERATOR}. Other product names on this site, including the
        coding agents {SITE.product} works with, belong to their owners. Their use here doesn't imply
        affiliation or endorsement.
      </p>

      <h2>Liability</h2>
      <p>
        To the extent the law allows, {OPERATOR} isn't liable for any loss that comes from using this site.
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
