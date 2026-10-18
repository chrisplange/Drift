# Drift website

The website for Drift: a front page, downloads, the user guide, release notes, the licence in
plain words, and the web app at `/app/`. It's a static site built with [Astro](https://astro.build)
and hosted on Cloudflare Pages. The **Website** workflow (`.github/workflows/website.yml`)
builds and deploys it on every merge to `main`, and whenever a release is published.

## Where things come from

| On the site | Comes from |
|---|---|
| User guide (`/guide/`) | `docs/guide/*.md`, read in place. Links between pages are rewritten (`src/lib/remark-guide-links.mjs`), so the same Markdown works on GitHub. The sidebar follows the order of the links in `docs/guide/README.md`. |
| Screenshots | `docs/guide/images/` |
| Release notes, download links | The GitHub releases API, when the site is built. The download buttons check again in the visitor's browser. |
| Web app (`/app/`) | `apps/web`, built by Trunk in the workflow |
| Logo | `website/scripts/make_logo.py`, from the same drawing as the app icon |
| Colours | The tokens at the top of `src/styles/global.css`, which follow `crates/drift-app/src/theme.rs` |
| Domain, repo | `src/site.config.mjs` |

The front page's scroll animation is `src/components/SketchScroll.astro`. Its timings are in
`PHASES` and the plate's strokes in `PLATE`. The drawing it turns into is `src/lib/woman.ts`, free-to-use
art supplied by the site's owner.

## Working on it

You need Node 24.

```powershell
cd website
npm install
npm run dev
```

Then open <http://localhost:4321>. `/app/` needs the web app built into `public/app` first. It
also needs `npm run build` and `npm run preview`, because the dev server doesn't serve the
app's `index.html`:

```powershell
cd apps\web
trunk build --release --cargo-profile web-release --public-url /app/ --dist ..\..\website\public\app
```

(In Git Bash, put `MSYS_NO_PATHCONV=1` in front of that command, or `/app/` gets turned into
a Windows path.)

## One-time setup: Cloudflare

These steps need your accounts and, for a domain, payment, so they're yours to do. Everything
else is automated.

1. **Make a Cloudflare account** at <https://dash.cloudflare.com/sign-up>. The free plan is
   enough.
2. **Make an API token.** Go to **My Profile → API Tokens → Create Token**, choose **Create
   Custom Token**, and give it:
   - Name: `Drift website deploy`
   - Permissions: **Account → Cloudflare Pages → Edit**
   - Account Resources: **Include → your account**

   Then **Continue to summary → Create Token**, and copy the token. Cloudflare shows it only
   once.
3. **Find your account ID.** Open **Workers & Pages**. The **Account ID** is in the right-hand
   column. Copy it.
4. **Give both to GitHub.** In the Drift repository, go to **Settings → Secrets and variables →
   Actions → New repository secret**, and add:
   - `CLOUDFLARE_API_TOKEN`: the token
   - `CLOUDFLARE_ACCOUNT_ID`: the account ID
5. **Deploy.** Merge to `main`, or open **Actions → Website → Run workflow**. The first run
   creates the Pages project, called `drift-website`. The site is then live at
   **https://drift-website.pages.dev**. If that name is taken, Cloudflare adds a suffix:
   the run's log and the Pages dashboard show the real address. To choose another name, add
   a repository *variable* (not a secret) `CLOUDFLARE_PROJECT`.
6. **Point the old address at the new site.** Once the site is up, add a repository variable
   `SITE_URL` set to its address, for example `https://drift-website.pages.dev`. On the next
   CI run, GitHub Pages (chrisplange.github.io/Drift/) redirects to the new `/app/`.

Until the secrets exist, the workflow still builds the site on every run, but it doesn't
deploy it.

## Later: a custom domain

1. Buy the domain. Cloudflare Registrar (**Domain Registration → Register Domains**) sells at
   cost, about $10 a year for a `.com`, and keeps everything in one account. Any registrar
   works.
2. In **Workers & Pages → drift-website → Custom domains → Set up a custom domain**, enter
   it. For a domain on Cloudflare, the DNS record is added for you.
3. Change `url` in `src/site.config.mjs` to the new address, and update the `SITE_URL`
   repository variable.
