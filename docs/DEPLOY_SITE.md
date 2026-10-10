# Deploying the public site (vibezeo.com)

The public site is the static export of `apps/web` (D-102): Home, About, Privacy, Terms, Security, a 404 page, `robots.txt`, `sitemap.xml` and `/.well-known/security.txt`. It needs no server, database or API, and is hosted on Vercel.

Vercel deploys it from `main`. Every push to `main` that changes `apps/web`, `packages/ui`, `packages/protocol`, the desktop interface (`apps/desktop/src`, `demo.html`, `vite.demo.config.ts`, `package.json`) or `pnpm-lock.yaml` updates the live site. Other pushes are skipped by the rule in `apps/web/vercel.json` (`ignoreCommand`).

**Which branches build.**
- Production builds `main`.
- Preview deployments are built only for `product-m0-m5` and `release-*` branches, the ones whose site someone checks before a release.
- Pushes to any other branch are skipped. Vercel lists them as "Canceled" with the message "Only main, product-m0-m5 and release-* deploy". Each feature branch's site is checked by its local gate instead.

**Why the rule is written as it is.**
- Vercel clones only the last few commits. When the commit of the last deployment isn't in that clone (after a large merge), the rule builds instead of letting `git diff` fail with "bad object".
- Many parallel branches would otherwise also use up the plan's daily deployment allowance.

**Node.** `apps/web/package.json` pins Node 22 (`engines`), which overrides the project's Node setting. The demo's Vite build needs Node 20.19+ or 22.12+.

The live demo at `/demo/app/` is the desktop interface built for the browser against a simulated core (`apps/desktop/src/demo`, D-126). `build:site` builds it first (`pnpm --filter desktop build:demo`, into `apps/web/public/demo/app/`, which git ignores), so Vercel installs the desktop app's dependencies too (`--filter desktop...`). It's the only page with scripts; they're first-party and make no network requests. Preview deployments (`product-m0-m5`, `release-*`) stay behind a Vercel login (Deployment Protection).

```bash
pnpm --filter web build:site     # writes apps/web/out
# exactly what Vercel runs, from apps/web:
pnpm install --frozen-lockfile --filter web... --filter desktop... && pnpm build:site
```

- **Content:** company, founder, contact email and optional legal details (registered name, CIN, address, city) live in `apps/web/lib/site.ts`.
- **Hosting config:** `apps/web/vercel.json` holds the build commands, response headers (CSP, framing, caching) and the skip rule.

## 1. The Vercel project (done once)

1. On vercel.com, go to **Add New… → Project** and import `NamanJain-Engineer/tandem`. If it isn't listed, choose **Adjust GitHub App Permissions** and grant Vercel access to that repository.
2. Vercel finds several apps in the repo. Next to **web** (Next.js), click **Import single project**. Don't use the multi-service import.
3. Check that **Root Directory** is `apps/web` and **Application Preset** is **Next.js**. Leave the build, output and install settings alone, because `vercel.json` sets them. Then click **Deploy**.
4. Under **Settings → Environments → Production → Branch Tracking**, the branch must be `main`.

Under **Settings → General**, check that **Node.js Version** is 22.x or later. Leave **Speed Insights** and **Web Analytics** off: the Privacy page promises no analytics.

## 2. Point vibezeo.com at it

Go to **Settings → Domains → Add**, enter `vibezeo.com`, and accept the suggestion to also add `www.vibezeo.com` redirecting to it. Vercel then shows the DNS records to create. Use the exact values it shows; they're usually:

| Type | Name | Value |
|---|---|---|
| A | `@` | `76.76.21.21` |
| CNAME | `www` | `cname.vercel-dns.com` |

Add them at your domain registrar's DNS settings. Alternatively, switch the domain's nameservers to Vercel's and it creates the records itself. The domain shows **Valid Configuration** once DNS has propagated, which takes minutes and sometimes a few hours. Vercel then issues the HTTPS certificate automatically.

## 3. Receive mail at hello@vibezeo.com

Vercel hosts websites, not mail. To receive mail at the site's contact address, use one of these:

- **Registrar forwarding:** most registrars forward `hello@vibezeo.com` to your inbox for free.
- **A forwarding service such as ImprovMX** (free tier): add the MX and TXT records it gives you, in the same place as the step 2 records.

Then send a test message to `hello@vibezeo.com` from another account and check that it arrives.

## The `vibezeo` repository

`NamanJain-Engineer/vibezeo` holds an earlier one-page site with a GitHub Pages workflow and a `CNAME` for vibezeo.com. Only one host can serve the domain. Remove the custom domain under that repo's **Settings → Pages**, or archive the repo, so it doesn't compete with Vercel.

## Check after deploying

```bash
curl -sI https://vibezeo.com/ | grep -i -E "^(HTTP|content-security-policy|x-frame-options)"
curl -s https://vibezeo.com/.well-known/security.txt
```

## Moving the site to its own repository later

The site is self-contained:

- `apps/web/app/**/*.site.tsx` and the three route folders (`robots.txt`, `sitemap.xml`, `.well-known`)
- `apps/web/components/site/` and `apps/web/lib/site.ts`
- `apps/web/public/` and `apps/web/vercel.json`
- from `packages/ui`: the design tokens, the fonts, and the `Badge` and `StatusDot` components

To move it, copy these into a new repo, or keep their history with `git subtree split --prefix=apps/web`. Then import the new repo in Vercel. To move the domain, remove it under **Settings → Domains** in the old project and add it in the new one.
