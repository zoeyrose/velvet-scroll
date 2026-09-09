# Project website

The site lives in `site/`: plain HTML, CSS and a small package-selection script.
It uses local assets and system fonts, with no analytics or framework runtime.
Downloads use the latest stable GitHub release, with a GitHub Releases fallback
when JavaScript or the API is unavailable.

## Local development

```sh
npm run site:build
python3 -m http.server 8080 --directory build/site
```

Rebuild after editing source files. The build only copies static files; it needs
Node.js but no npm dependencies. Check mobile and desktop layouts. Security
headers live in `site/_headers` and are copied into the output for Cloudflare.
The existing required GitHub CI check also builds the site.

## Native Cloudflare Pages integration

Connect `zoeyrose/velvet-scroll` through the **Cloudflare Workers & Pages** GitHub
App. Grant access to this repository in GitHub Settings → Applications →
Installed GitHub Apps. Create a Pages project using **Connect to Git**, with:

| Setting | Value |
| --- | --- |
| Project name | `velvet-scroll` |
| Production branch | `main` |
| Framework preset | None |
| Root directory | Repository root |
| Build command | `node scripts/build-site.mjs` |
| Build output directory | `build/site` |
| Environment variable, production and preview | `NODE_VERSION=24` |
| Environment variable, production and preview | `SKIP_DEPENDENCY_INSTALL=true` |
| Preview branches | All non-production branches |
| PR comments | Enabled |

Cloudflare builds and deploys directly from Git. No GitHub Actions deployment
workflow, Wrangler dependency, Cloudflare API token or account variable is
needed. Keep Cloudflare build environments free of application secrets.

Merging a PR into `main` triggers a production deployment. Pushing branches in
this repository triggers preview deployments, with URLs/checks attached by
Cloudflare. Native GitHub integration does **not** deploy fork PRs; external
contributions need maintainer review and a branch in this repository to preview.
Repository write permission controls who can push these branches. Cloudflare
builds run independently of GitHub CI; the existing required checks protect
merges to main.

Preview URLs are public to view and marked `noindex`. To require login, enable
Cloudflare Access for Pages previews and configure the desired identity allowlist.
Branch deployment permissions and preview viewing permissions are separate.

The first production build requires the website PR to be merged. Before that,
the website branch can be built as a preview. Retry failed builds through the
Cloudflare deployment page. Revert a change through a PR to roll back production.

## Domains

Attach `velvet-scroll.com` and `velvet-scroll.org` in Pages → Custom domains.
The `.com` address is canonical. In each zone, ensure a proxied apex CNAME:

| Zone | Type | Name | Target |
| --- | --- | --- | --- |
| `velvet-scroll.com` | CNAME | `@` | `velvet-scroll.pages.dev` |
| `velvet-scroll.org` | CNAME | `@` | `velvet-scroll.pages.dev` |

Keep unrelated DNS records, including mail records. Check that both custom domains
show Active. If Cloudflare assigns a different Pages subdomain, use that hostname
as the CNAME target instead.

Optionally redirect `.org` to `.com` with a Cloudflare Single Redirect matching
hostname `velvet-scroll.org`, dynamic target
`concat("https://velvet-scroll.com", http.request.uri.path)`, status 301, and
Preserve query string enabled. Without it, both domains serve the same site.

References: [GitHub integration](https://developers.cloudflare.com/pages/configuration/git-integration/github-integration/),
[build configuration](https://developers.cloudflare.com/pages/configuration/build-configuration/),
and [preview deployments](https://developers.cloudflare.com/pages/configuration/preview-deployments/).
