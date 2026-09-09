# Project website

The site lives in `site/`. It is plain HTML, CSS and a small script for package
selection. It uses local assets, system fonts and no analytics, cookies or
framework runtime. The download chooser reads the latest stable GitHub release;
if the API is unavailable or JavaScript is disabled, the GitHub Releases link
still works. Installation details stay in the repository documentation.

## Local development

```sh
npm run site:build
python3 -m http.server 8080 --directory build/site
```

Open `http://localhost:8080`. Rebuild after editing source files. Deployment
security tests run with `npm --prefix site-ci test`; they do not require any
Cloudflare credentials.

## Cloudflare setup

The dedicated **velvet-scroll** Pages Direct Upload project uses `main` as its
production branch and `velvet-scroll.pages.dev` as its default hostname.
`velvet-scroll.com` is the primary canonical address. Both `.com` and `.org`
are attached as custom domains.

GitHub Actions uses:

| Setting | Location | Purpose |
| --- | --- | --- |
| `CLOUDFLARE_ACCOUNT_ID` | Repository Actions variable | The account containing the Pages project |
| `CLOUDFLARE_API_TOKEN` | Repository Actions secret | Dedicated token with Account → Cloudflare Pages → Edit, scoped to that account |

The account variable is configured. Add the dedicated API token through GitHub's
repository **Settings → Secrets and variables → Actions**. Do not put it in a
commit, issue, PR comment or chat. The local Wrangler OAuth session is useful for
one-time setup, but is not copied into CI.

The current local OAuth login cannot read or edit DNS records. In each domain's
Cloudflare DNS settings, ensure the apex has a proxied CNAME:

| Zone | Type | Name | Target |
| --- | --- | --- | --- |
| `velvet-scroll.com` | CNAME | `@` | `velvet-scroll.pages.dev` |
| `velvet-scroll.org` | CNAME | `@` | `velvet-scroll.pages.dev` |

Replace conflicting apex A/AAAA records if present; keep unrelated records such
as mail records. Cloudflare flattens apex CNAMEs. Check Pages → velvet-scroll →
Custom domains until both show **Active**.

To send `.org` visitors to the canonical `.com` address, add a Cloudflare Single
Redirect in the `.org` zone, matching hostname `velvet-scroll.org`, with a dynamic
target `concat("https://velvet-scroll.com", http.request.uri.path)`, status 301,
and **Preserve query string** enabled. Domain redirects are Cloudflare settings,
not a Pages `_redirects` rule. If the redirect is omitted, both domains serve the
same site and its canonical link points to `.com`.

## Production and preview deployments

**Website checks** runs on main pushes and PRs targeting main. It builds static
files without Cloudflare credentials, tests the deployment tooling, and uploads
a run-attempt-specific artifact. Main pushes happen when PRs are squash merged.

**Website deploy** runs only after successful Website checks. All privileged
scripts and the pinned Wrangler dependency come from trusted `main`. It uses
fresh GitHub API responses to verify the run, artifact digest and source SHA:

- Production must still match main's current HEAD.
- A preview must belong to an open PR targeting main and match its current HEAD.
- Both the PR author and the build's triggering actor must currently have write,
  maintain or admin access to this repository. This also supports authorized
  contributors working from forks. An untrusted author's PR is not authorized
  merely because a maintainer reruns its checks.
- Stale, failed, ambiguous and unauthorized builds do not deploy.

The gate is checked again immediately before deployment. The downloaded artifact
is size bounded and extracted through a strict static-file allowlist. Symlinks,
path traversal, Pages Functions, Workers and deployment configuration are
rejected. Security headers are generated from trusted code. No PR build scripts
run in the job holding Cloudflare credentials.

Production uses the `main` Pages branch; PRs use `pr-N`. GitHub deployment statuses
show the exact deployment URL on the PR. Preview URLs are public to view; the
permission gate controls who can deploy them. Cloudflare marks previews noindex.
For private viewing, enable Cloudflare Access for the Pages preview hostnames and
configure an identity allowlist separately; this workflow does not infer who
should be allowed to sign in.

The deployment workflow must first be merged into main before GitHub will run
its `workflow_run` trigger. The initial website PR can be reviewed using a
one-time Pages preview made by the maintainer; subsequent authorized PR previews
are automatic. No production deployment from an unmerged PR is performed.

For a failed deployment after configuration is corrected, rerun **Website
deploy** while its source is still current. Otherwise rerun the latest Website
checks. To roll back production, revert the website change through a PR so the
build, checks and deployment remain tied to reviewed source.
