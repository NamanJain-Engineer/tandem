# Deploying the public site (vibezeo.com)

The public site is the static export of `apps/web` (D-102): Home, About, Privacy, Terms, Security, a 404 page, `robots.txt`, `sitemap.xml` and `/.well-known/security.txt`. It needs no server, database or API, and is hosted on Vercel.

```bash
pnpm --filter web build:site     # writes apps/web/out
```

- **Content:** company, founder, contact email and optional legal details (registered name, CIN, address, city) live in `apps/web/lib/site.ts`.
- **Hosting config:** `apps/web/vercel.json` holds the build commands, response headers (CSP, framing, caching) and the skip rule.
- **Skip rule:** Vercel only rebuilds when `apps/web`, `packages/ui` or the lockfile changed since the last deployment. Desktop-only commits don't redeploy the site.

## 1. Create the Vercel project (once)

1. Sign in at vercel.com with GitHub, then **Add New… → Project**.
2. Under **Import Git Repository**, pick `NamanJain-Engineer/tandem`. If it isn't listed, choose **Adjust GitHub App Permissions** and grant Vercel access to that repository. It stays private.
3. On the configure screen:

   | Setting | Value |
   |---|---|
   | Project name | `vibezeo` (any name; it becomes `vibezeo.vercel.app`) |
   | Root Directory | `apps/web` (click **Edit** and pick the folder) |
   | Framework Preset | Next.js (detected) |
   | Build, install and output settings | leave as they are: `vercel.json` sets them |

4. Click **Deploy**. This first build runs from the default branch (`main`), which doesn't have the site yet, so it's expected to fail or show an old page.
5. Go to **Settings → Git → Production Branch** and set it to `ccr-6d871eba-mxry64`. Merge it into `main` later and switch back.
6. Go to **Deployments**, then **Create Deployment** (or **Redeploy**) and pick branch `ccr-6d871eba-mxry64`. The build takes about two minutes. Check the result at `https://vibezeo.vercel.app`.

Under **Settings → General**, make sure **Node.js Version** is 22.x.

## 2. Point vibezeo.com at it

Go to **Settings → Domains → Add**, enter `vibezeo.com`, and accept the suggestion to also add `www.vibezeo.com` redirecting to it. Vercel then shows the DNS records to create. Use the exact values it shows, which are usually these:

| Type | Name | Value |
|---|---|---|
| A | `@` | `76.76.21.21` |
| CNAME | `www` | `cname.vercel-dns.com` |

Add these at your domain registrar's DNS settings. Or switch the domain's nameservers to Vercel's, and Vercel creates the records itself. The domain shows **Valid Configuration** once DNS has propagated (minutes, sometimes a few hours), and Vercel then issues the HTTPS certificate automatically.

## 3. Receive mail at hello@vibezeo.com

Vercel hosts websites, not mail. Two ways to receive mail at the site's contact address:

- **Forwarding at your registrar.** Most registrars offer free forwarding: `hello@vibezeo.com` → your inbox.
- **A forwarding service such as ImprovMX** (free tier). Add the MX and TXT records it gives you in the same DNS place as step 2.

Send a test message to `hello@vibezeo.com` from another account and check that it arrives.

## 4. Updates

Every push to the production branch that touches the site redeploys it. Other branches get preview URLs.

## The `vibezeo` repository

`NamanJain-Engineer/vibezeo` holds an earlier one-page site with a GitHub Pages workflow and a `CNAME` for vibezeo.com. Only one host can serve the domain. Remove the custom domain under that repo's **Settings → Pages**, or archive the repo, so it doesn't compete with Vercel.

## Check after deploying

```bash
curl -sI https://vibezeo.com/ | grep -i -E "^(HTTP|content-security-policy|x-frame-options)"
curl -s https://vibezeo.com/.well-known/security.txt
```

## Moving the site to its own repository later

The site is self-contained:
- `apps/web/app/**/*.site.tsx` and the three route folders (`robots.txt`, `sitemap.xml`, `.well-known`);
- `apps/web/components/site/` and `apps/web/lib/site.ts`;
- `apps/web/public/` and `apps/web/vercel.json`;
- the design tokens and the `Badge` and `StatusDot` components from `packages/ui`.

To move it, copy those into a new repo, or keep history with `git subtree split --prefix=apps/web`. Then re-import the new repo in Vercel and move the domain: under **Settings → Domains** in the old project, remove it, then add it to the new one.
