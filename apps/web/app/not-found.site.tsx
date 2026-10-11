import { buttonClass } from "@tandem/ui/components/Button";
import { PageHero } from "../components/site/PageHero";
import { SiteFrame } from "../components/site/SiteFrame";
import { TextLink } from "../components/site/TextLink";

export default function NotFound() {
  return (
    <SiteFrame>
      <PageHero eyebrow="404" title="This page doesn't exist" lead="The link may be old, or the address mistyped.">
        <div className="flex flex-wrap items-center gap-x-6 gap-y-3 pt-2">
          <a href="/" className={buttonClass("primary", "xl")}>
            Go to the home page
          </a>
          <TextLink href="/docs/">Read the docs</TextLink>
          <TextLink href="/download/">Download</TextLink>
        </div>
      </PageHero>
    </SiteFrame>
  );
}
