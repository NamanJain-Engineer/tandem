import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import type { ComponentProps } from "react";
import { DocsFooter } from "../../../components/docs/DocsFooter";
import { DocsShell } from "../../../components/docs/DocsShell";
import { DOCS_COMPONENTS } from "../../../components/docs/Generated";
import { source } from "../../../lib/docs/source";
import { withSlashes } from "../../../lib/docs/tree";

/** Links inside the docs open in the page (D-1817); others are plain. */
function A({ href = "", children, className, id }: ComponentProps<"a">) {
  return href.startsWith("/docs/") ? (
    <Link href={href} className={className} id={id}>
      {children}
    </Link>
  ) : (
    <a href={href} className={className} id={id}>
      {children}
    </a>
  );
}

type Props = { params: Promise<{ slug?: string[] }> };

export const dynamicParams = false;

export function generateStaticParams() {
  return source.generateParams();
}

export async function generateMetadata({ params }: Props): Promise<Metadata> {
  const page = source.getPage((await params).slug);
  if (!page) notFound();
  return {
    title: page.data.title,
    description: page.data.description,
    alternates: { canonical: page.url + "/" },
  };
}

export default async function DocPage({ params }: Props) {
  const page = source.getPage((await params).slug);
  if (!page) notFound();
  const Body = page.data.body;
  return (
    <DocsShell tree={withSlashes(source.pageTree)} url={page.url.endsWith("/") ? page.url : `${page.url}/`} toc={page.data.toc}>
      <h1 className="docs-title">{page.data.title}</h1>
      {page.data.description && <p className="docs-lead">{page.data.description}</p>}
      <Body components={{ a: A, ...DOCS_COMPONENTS }} />
      <DocsFooter />
    </DocsShell>
  );
}
