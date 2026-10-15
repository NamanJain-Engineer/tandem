import { readFileSync } from "node:fs";
import { join } from "node:path";
import { ImageResponse } from "next/og";
import { OG_SIZE, POSTS, formatDate } from "../../../../lib/blog/posts";
import { SITE } from "../../../../lib/site";

// Each post's share image (F-WB-06, D-1859): drawn at build time from its
// title, tags and date, in Vibezeo's type and colours. No cover needed.

export const dynamic = "force-static";
export const dynamicParams = false;

export function generateStaticParams(): Array<{ slug: string }> {
  return POSTS.map((p) => ({ slug: p.slug }));
}

const font = (file: string) => readFileSync(join(process.cwd(), "assets", "og", file));

export async function GET(_req: Request, { params }: { params: Promise<{ slug: string }> }): Promise<Response> {
  const { slug } = await params;
  const post = POSTS.find((p) => p.slug === slug);
  if (!post) throw new Error(`No post ${slug}`);
  // The site's dark stage: the faint grid and soft teal glow of /og.png.
  const backdrop = [
    "radial-gradient(60% 70% at 18% 0%, rgba(45,212,191,0.22), rgba(6,10,18,0) 70%)",
    "linear-gradient(rgba(148,163,184,0.07) 1px, rgba(0,0,0,0) 1px)",
    "linear-gradient(90deg, rgba(148,163,184,0.07) 1px, rgba(0,0,0,0) 1px)",
  ].join(", ");
  return new ImageResponse(
    (
      <div
        style={{
          width: "100%",
          height: "100%",
          display: "flex",
          flexDirection: "column",
          justifyContent: "space-between",
          padding: 72,
          backgroundColor: "#060a12",
          backgroundImage: backdrop,
          backgroundSize: "100% 100%, 48px 48px, 48px 48px",
          color: "#e6edf5",
          fontFamily: "Inter",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: 16, fontSize: 30, fontWeight: 600 }}>
          <svg width="44" height="44" viewBox="0 0 28 28" fill="none">
            <circle cx="10.5" cy="14" r="7" stroke="#2dd4bf" strokeWidth="2.2" />
            <circle cx="17.5" cy="14" r="7" stroke="#2dd4bf" strokeWidth="2.2" />
          </svg>
          {SITE.product}
          <span style={{ color: "#64748b", fontWeight: 400 }}>/ blog</span>
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 28 }}>
          <div style={{ width: 96, height: 6, borderRadius: 3, backgroundImage: "linear-gradient(90deg, #2dd4bf, #38bdf8)" }} />
          <div style={{ display: "flex", fontSize: post.title.length > 40 ? 64 : 84, fontWeight: 600, letterSpacing: -2.5, lineHeight: 1.04, maxWidth: 1000 }}>
            {post.title}
          </div>
          <div style={{ display: "flex", fontSize: 30, lineHeight: 1.4, color: "#9aa8bb", maxWidth: 980 }}>
            {post.description.length > 150 ? `${post.description.slice(0, post.description.lastIndexOf(" ", 147))}…` : post.description}
          </div>
        </div>
        <div style={{ display: "flex", justifyContent: "space-between", fontSize: 24, color: "#9aa8bb" }}>
          <span>{post.author}</span>
          <span>{formatDate(post.date)}</span>
        </div>
      </div>
    ),
    {
      ...OG_SIZE,
      fonts: [
        { name: "Inter", data: font("inter-regular.ttf"), weight: 400, style: "normal" },
        { name: "Inter", data: font("inter-semibold.ttf"), weight: 600, style: "normal" },
      ],
    },
  );
}
