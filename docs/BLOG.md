# The blog (F-WB-06)

vibezeo.com/blog/ is where the owner publishes articles about Vibezeo. This file has
three parts:
- what we learned from studying the blogs of other developer-tool and AI products;
- the plan we built from it;
- how to publish a post.

The research names the sites we studied. Nothing on our blog may copy their copy,
code, assets, typefaces or signature effects (`docs/WEBSITE_BRIEF.md` §1.2 on
`docs-briefs`, DECISIONS "Identity"). We record patterns only.

## 1. What we studied (15 October)

We fetched the index and one or two recent posts from each of these blogs:
- Linear (`/now`);
- Vercel;
- Raycast;
- Warp;
- Zed;
- Cursor;
- Supabase;
- Resend;
- Tailwind CSS;
- Anthropic's engineering posts.

BridgeMind's site blocks automated visitors, and a search found no blog for it.

### The index
- **One lead post, then a quieter list.** Most of these blogs feature one to three
  posts with a cover (Raycast, Warp, Cursor, Supabase, Zed and Anthropic). The rest
  are text-first rows: title, a one-line description, a date and sometimes a
  category. Thumbnails on every row are the exception (Linear, Resend).
- **Categories are links or tabs** (Linear, Warp, Cursor, Supabase and Zed). When
  each category is its own pre-rendered URL, it needs no script.
- **Card metadata** is a date, often a reading time (Warp, Cursor and Supabase),
  and author avatars on some.
- **Paging** is a single long stream or a "Load more" button. None of these blogs
  numbers its pages.

### The post page
- **The title block comes in one order:** a category or breadcrumb, a large H1
  with tight leading and balanced wrapping, a one or two sentence lede, then one
  quiet line with the author, the date and the reading time. Vercel moves the date
  and reading time into a side rail. Anthropic puts the date under the title and
  the credits at the end.
- **Authors are people.** Most show a name with a round avatar. Linear links the
  names instead.
- **Dates** are spelled out in full ("12 March"-style) inside `<time datetime>`.
- **A narrow reading column**, about 65–75 characters (640–720 px), with large type
  around it. Some figures break out wider than the text (Resend).
- **Covers** are optional. Cursor and Anthropic open with the title and put the
  first image after the lede. A post without an image still looks finished, as
  long as the title block carries the page.
- **Headings** have anchor links (Raycast, Zed, Cursor and Tailwind). Long posts
  get an "On this page" contents list: a sticky side rail on Zed and Supabase, and
  an inline list after the lede on Tailwind. Highlighting the section you're
  reading needs a script; the list itself doesn't.
- **Figures have captions** that state the point of the image (Anthropic, Zed and
  Linear).
- **Code** is highlighted at build time (Shiki or Prism). Zed and Tailwind use
  light and dark themes, a mono font around 13–14 px, a thin border and a file
  name label. Copy buttons need a script.
- **Footnotes and callouts are rare.** Anthropic collects footnotes at the end,
  and Warp uses one callout.
- **Navigation after the post:**
  - previous and next cards (Supabase);
  - two or three related posts (Cursor, Warp and Zed);
  - "More posts" or "View all" (Vercel and Warp).

### Subscribing
- RSS or Atom is linked in `<head>` with `<link rel="alternate">`, and often from
  an icon on the index too:
  - Atom: Vercel and Raycast;
  - RSS: Linear, Supabase, Resend and Zed;
  - Tailwind offers RSS, Atom and JSON Feed.
- Newsletters sit in the footer or at the end of a post. We don't add one (below).
- Supabase and Resend also publish each post as Markdown, for agents.

### Search engines and sharing
- Every blog sets a canonical URL. Most give each post its own `og:image` (its cover,
  or a generated title card).
- JSON-LD is less common than expected. Warp (Article and BlogPosting), Supabase,
  Resend, Cursor and Anthropic (BlogPosting) ship it. Linear, Vercel, Raycast and
  Zed don't.

### What makes them feel premium
- Restraint: one accent colour, a tight grey scale, very little chrome inside the
  post, and generous vertical space.
- Type does the work: a confident display H1, a calm body size and a narrow measure.
- Real product images with captions, instead of decoration.
- Every detail is finished: balanced headings, anchors, honest reading times,
  dates in full, and feeds in the head.
- Small pages that load fast. Raycast's post is about 57 KB of HTML.

## 2. The plan for Vibezeo's blog

Built from the patterns above, in Vibezeo's own identity: Inter and JetBrains
Mono, the teal accent, slate neutrals, 8 px panels and 1 px borders. The styling
lives in `components/blog/**`, on top of the shared site components and tokens.

### Pages
| URL | What it shows |
|---|---|
| `/blog/` | The newest post featured: its cover if it has one, otherwise a typographic panel. Then every other post as a row: date, title, description, tags and reading time. Then a row of tag links and the ways to subscribe. With no posts, an empty state that points to the feed and the changelog. |
| `/blog/<slug>/` | The back link and tags, the H1, the description as the lede, then the author's monogram, name, date and reading time. Then the optional cover with its caption, and the body in a 68-character column. On wide screens, a sticky "On this page" list sits in the left rail (no script). At the end: the previous and next posts, the ways to subscribe and "All posts". |
| `/blog/tags/<tag>/` | Every post with that tag, as the index's rows. |
| `/blog/feed.xml` | An Atom feed of every published post, with the full description and a link. |

### Rules
- **No client JavaScript.** Pages are server components, and `strip-runtime`
  removes Next's runtime as on the other marketing pages. The one script allowed
  is a post's JSON-LD (`BlogPosting`): data, not code, allowed on post pages only
  (D-1852).
- **Code** is highlighted at build time by fumadocs-mdx's Shiki, with light and
  dark themes. There's no copy button, since that needs a script.
- **Dates and the no-year rule (D-1851).** Dates appear only inside `<time datetime>`
  and in the JSON-LD's `datePublished`. The year-free check runs on every blog
  page with those two removed.
- **Subscribing** is the Atom feed or an email to contact@vibezeo.com. There's no
  newsletter form, which would send visitors' addresses to a third party.
- **First-party only:** no embeds, no third-party images or fonts, no cookies or
  storage, and the CSP unchanged. Link out to videos instead of embedding them.
- **Images** live under `public/blog/<slug>/`, at fixed sizes, with AVIF and WebP,
  and lazy unless they're the cover. A cover from a real capture names the build
  it shows.
- **Share images:** a post's `og:image` is its cover, or `/og.png`.
- **Search engines:** a canonical URL on every page, the feed in `<head>`, every
  blog URL in the sitemap, and the posts in `/llms.txt`.

## 3. Publishing a post

You need no code to publish a post: you add one file and merge it.

1. **Create the file.** From the repository root:
   ```bash
   pnpm --filter web new-post "Your title here"
   ```
   This writes `apps/web/content/blog/your-title-here.mdx` with the frontmatter
   filled in and `draft: true`. Or create the file by hand. The file name is the
   URL: `/blog/your-title-here/`. Use lowercase letters, digits and hyphens. A
   plain `.md` file works too.
2. **Fill in the frontmatter**, between the two `---` lines at the top:
   ```yaml
   ---
   title: Your title here
   description: One or two sentences. They are the lede, the card text, the feed summary and the share text.
   date: 2026-10-15        # the publication date, YYYY-MM-DD
   author: Naman Jain
   tags: [release, linux]  # lowercase words or hyphenated phrases; each gets a page
   cover:                  # optional
     src: /blog/your-title-here/cover.png
     alt: What the image shows, for screen readers
     width: 1440
     height: 900
     caption: Optional caption. A capture of the app names the build it shows.
   draft: true             # optional; drafts show only in `pnpm --filter web dev`
   ---
   ```
   The build checks every field. A missing title, a date that isn't
   `YYYY-MM-DD`, a tag with capitals or a cover file that doesn't exist all stop
   the build, and the message names the file and the field.
3. **Write the post** in Markdown below the frontmatter. You can use:
   - `##` and `###` headings, which build the "On this page" list;
   - lists, tables and links;
   - fenced code blocks with a language (```` ```bash ````), highlighted at build time;
   - `> ` quotes;
   - images (next step).

   Don't write a year in the text: the site's tests reject years outside the post's
   date. Write "15 October" or "version 0.7.2" instead. Link to videos rather than
   embedding them.
4. **Add images** under `apps/web/public/blog/<slug>/`, as a PNG or JPEG
   1440 px wide (or 2880 for sharp text), then make the AVIF and WebP copies:
   ```bash
   python3 apps/web/scripts/optimize-screens.py
   ```
   In the post, use the `Figure` component so the image gets its size, formats,
   lazy loading and caption:
   ```mdx
   <Figure src="/blog/<slug>/panes.png" width={1440} height={900}
     alt="What it shows" caption="What the reader should notice." />
   ```
   Plain Markdown images (`![alt](/blog/<slug>/x.png)`) also work, with their
   size read from the file, but without the AVIF and WebP copies or a caption.
   For a code block with a file name bar, add a title to the fence:
   ```` ```bash title="install.sh" ````.
5. **Preview it.** Run `pnpm --filter web dev` and open
   http://localhost:3000/blog/. Drafts show there. Before you publish, delete the
   `draft: true` line, then run `pnpm --filter web build:site` to check it the way
   the live site builds it.
6. **Publish it.** Commit the post and its images on a branch and merge it into
   `product-m0-m5`. The site deploys when that is merged into `main` (OPERATING
   §7). The post then appears on the index, its tags, the feed, the sitemap and
   `/llms.txt`.
