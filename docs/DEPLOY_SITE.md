# Deploying the public site (vibezeo.com)

The public site is the static export of `apps/web` (D-102): home, Privacy, Terms, Security, a 404 page, `robots.txt`, `sitemap.xml` and `/.well-known/security.txt`. It needs no server, database or API, and is hosted on Cloudflare Pages.

```bash
pnpm --filter web build:site     # writes apps/web/out
```

Contact and company details, and the optional founder line, live in `apps/web/lib/site.ts`. Response headers (CSP, framing, caching) are in `apps/web/public/_headers`, which Cloudflare Pages applies.

## 1. Create the Pages project (once)

1. In the Cloudflare dashboard, go to **Workers & Pages → Create → Pages → Connect to Git**.
2. Authorize the Cloudflare GitHub app for `NamanJain-Engineer/tandem`. The repo is private, so grant it access to that repository.
3. Pick the repository, then fill in the build settings:

   | Setting | Value |
   |---|---|
   | Project name | `tandem-site` (any name; it becomes `tandem-site.pages.dev`) |
   | Production branch | `ccr-6d871eba-mxry64` (switch to `main` once the work is merged) |
   | Framework preset | None |
   | Build command | `pnpm --filter web build:site` |
   | Build output directory | `apps/web/out` |
   | Root directory | *(leave empty: the repo root)* |

4. Under **Environment variables (advanced)**, add these:

   | Variable | Value |
   |---|---|
   | `NODE_VERSION` | `22` |
   | `PNPM_VERSION` | `9.15.9` |

   Cloudflare installs dependencies itself when it sees `pnpm-lock.yaml`. `PNPM_VERSION` makes it use the same pnpm as the lockfile.
5. **Save and Deploy.** The first build takes about two minutes. Check the result at `https://tandem-site.pages.dev`.

## 2. Point vibezeo.com at it

In the Pages project, go to **Custom domains → Set up a custom domain**. Add `vibezeo.com`, then add `www.vibezeo.com` the same way.

- **If vibezeo.com's DNS is on Cloudflare** (its nameservers are Cloudflare's), Cloudflare creates the records for you. The domain goes live once the certificate is issued, usually within a few minutes.
- **If the domain is at another registrar's DNS,** first add the site to Cloudflare (**Add a site → vibezeo.com**, Free plan). Then change the nameservers at your registrar to the two Cloudflare gives you, and add the custom domains as above. Pages can only serve an apex domain (`vibezeo.com` without `www`) on a Cloudflare zone. If you only want `www`, you can instead add a `CNAME www → tandem-site.pages.dev` record at your registrar.

Optional: to send `www` to the apex, go to **Rules → Redirect Rules** and add a rule. When the hostname equals `www.vibezeo.com`, set a dynamic redirect to `concat("https://vibezeo.com", http.request.uri.path)` with status 301.

## 3. Receive mail at hello@vibezeo.com

The site's contact address is `hello@vibezeo.com`. Cloudflare Email Routing forwards it to your own inbox for free. It needs the domain's DNS on Cloudflare (step 2).

1. In the vibezeo.com zone, go to **Email → Email Routing → Get started**.
2. Create the custom address `hello` and set its destination to your own inbox. Cloudflare emails that inbox a verification link: open it.
3. Let Cloudflare add the MX and SPF (TXT) records it proposes.
4. Send a test message to `hello@vibezeo.com` from another account, and check that it arrives.

Replies go out from your own address. To reply as `hello@vibezeo.com`, add it as a "Send mail as" address in your mail client through an SMTP provider.

## The `vibezeo` repository

`NamanJain-Engineer/vibezeo` holds an earlier one-page site with a GitHub Pages workflow and a `CNAME` for vibezeo.com. Only one host can serve the domain. With Cloudflare Pages serving it, remove the custom domain under that repo's **Settings → Pages**, or archive the repo, so the two don't compete.

## 4. Updates

Every push to the production branch redeploys the site. Pushes to other branches get preview URLs.

## Without Git integration (fallback)

Build locally, then upload the output with Wrangler:

```bash
pnpm --filter web build:site
npx wrangler pages deploy apps/web/out --project-name tandem-site
```

## Check after deploying

```bash
curl -sI https://vibezeo.com/ | grep -i -E "^(HTTP|content-security-policy|x-frame-options)"
curl -s https://vibezeo.com/.well-known/security.txt
```
