import { SiteFrame } from "../components/site/SiteFrame";

export default function NotFound() {
  return (
    <SiteFrame>
      <section className="mx-auto flex max-w-3xl flex-col items-start gap-4 px-6 py-24">
        <p className="font-mono text-sm text-fg-subtle">404</p>
        <h1 className="text-2xl font-semibold tracking-tight">This page doesn't exist</h1>
        <p className="text-md text-fg-muted">The link may be old, or the address mistyped.</p>
        <a
          href="/"
          className="rounded-sm bg-accent px-4 py-2 text-md font-medium text-accent-fg hover:bg-accent-hover"
        >
          Go to the home page
        </a>
      </section>
    </SiteFrame>
  );
}
