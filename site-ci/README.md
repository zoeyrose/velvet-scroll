# Website deployment

`Website checks` builds `build/site/` without deployment credentials. Its
`website-static-N` artifact is tied to the GitHub run and attempt number.
`Website deploy` starts only after that workflow succeeds, and reads all scripts
and the pinned Wrangler lockfile from current `main`.

Production deploys only a successful push whose SHA is still `main`'s HEAD.
Previews require an open PR targeting `main`, its current head SHA and repository,
and current GitHub write/maintain/admin permission for both the PR author and the
account that triggered the checks run. GitHub's API supplies this information;
commit authors, artifact contents, and PR comments cannot authorize deployment.
Stale or untrusted runs are skipped. Authorization is checked again before upload.

The artifact is downloaded by its exact server-listed ID and SHA-256 digest.
The ZIP extractor rejects traversal, links, duplicate paths, excessive sizes,
Workers, Functions, configuration, and unsupported file types. It writes only
static files and replaces `_headers` with the policy in trusted `extract.py`.
Wrangler runs outside both the checkout and the extracted directory, using the
local, pinned tool and no GitHub credential. GitHub deployment statuses expose
the successful production or PR preview URL without posting comments.

Configure repository secret `CLOUDFLARE_API_TOKEN` with Cloudflare Pages Edit for
the target account, and repository variable `CLOUDFLARE_ACCOUNT_ID`. The Pages
project is `velvet-scroll`, with production branch `main`; previews use `pr-N`.
These checks control who can publish a preview. Preview URLs are public unless
Cloudflare Access is separately enabled for preview visitors.

Run security tests with `npm --prefix site-ci test`; they need Node and Python,
and use no credentials or network. A first PR introducing these workflows needs
a separately authorized manual preview: GitHub activates `workflow_run` from the
default branch only after merge.
