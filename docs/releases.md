# Builds and releases

Every pushed branch commit and pull request runs tests and builds Debian `.deb`,
RPM `.rpm`, Arch `.pkg.tar.zst`, and portable `.tar.gz` packages, natively on
x86_64 and aarch64 runners. Download development packages from the run's
`packages-*` artifacts in GitHub Actions (retained for 30 days). A push containing
several commits builds its tip; GitHub does not send a separate push event for
each historical commit. Failed builds do not publish releases.

## Version policy

The pinned semantic-release dependency computes versions with our custom policy:

- The first automated release is `1.0.0`.
- Each subsequent ordinary merge to `main` advances the minor: `1.1.0`, `1.2.0`,
  and so on. A fix or docs change on main also advances the minor.
- A maintenance branch named `1.1.x`, based on `v1.1.0`, advances only its patch:
  `1.1.1`, `1.1.2`, etc., even after main has reached `1.50.0`.
- Maintenance rejects feature commits and breaking-change markers. Backport
  compatible fixes; do not merge newer main history into a maintenance branch.
- `!` and `BREAKING CHANGE` never select a major release on main.
- Other branches and PRs build development versions such as
  `1.1.0-dev.42.gabcdef123456`; they do not create stable tags or releases.

Release versions live in Git tags, not in periodically edited source files.
Cargo.toml omits the package version; Cargo.lock retains Cargo's fixed `0.0.0`
placeholder. The build script and package builders share `scripts/version.sh`:
CI supplies `PACKAGE_VERSION`, tagged source builds use the exact `vN.N.N` tag,
and other checkouts produce a development version with their commit hash.
No release commit edits either Cargo file. Dependency versions remain locked.

Build a release from its source tag with normal commands:

```sh
git checkout v1.1.0
cargo build --release --locked
make packages
```

A source archive without Git metadata needs an explicit version, for example
`PACKAGE_VERSION=1.1.0 cargo build --release --locked`. Node is needed only for
release planning and its tests, not ordinary Cargo or package builds.

## Maintenance releases

Create the maintenance line once, then submit fixes through pull requests:

```sh
git fetch origin --tags
git switch -c 1.1.x v1.1.0
git push -u origin 1.1.x
git switch -c fix/1.1-scroll-timing
# Apply a compatible correction and commit it using a fix: title.
git push -u origin fix/1.1-scroll-timing
gh pr create --base 1.1.x --title 'fix: correct scroll timing'
```

Merging that PR publishes `1.1.1`. Maintenance releases do not replace the latest
main release on the GitHub Releases page. Newly creating a maintenance branch at
an existing release tag does not invent a patch release.

## Deliberate major releases

Only a maintainer should perform these steps:

1. Merge a PR whose squash commit title is exactly `chore(release): major 2`
   (substitute the next major number). This marker's push builds a development
   snapshot and reserves stable publication for the explicit action.
2. While that commit is still main's HEAD, run **Build and release** manually on
   `main`, setting `target_major` to `2`.

The gate checks the triggering account's admin/maintain permission through the
GitHub API, the exact dispatched commit and marker, and that the target is the
next major. Author names/emails and conventional breaking-change syntax cannot
satisfy this gate. Ordinary retries leave `target_major` empty.

## Publication and recovery

Release runs share a queue across main and maintenance branches. GitHub permits
up to 100 pending runs. Planning uses semantic-release against a temporary local
mirror pinned to the event commit, so a later push does not silently substitute
its source. Both architectures must build successfully before publishing.

Artifacts carry their version, source SHA, architecture and SHA-256 checksums.
The publisher verifies them before creating an immutable tag and a draft release,
then uploads all eight packages and publishes the complete release. `SHA256SUMS`
and `release.json` accompany the packages. No release bot commit or branch
protection bypass is needed. Rerunning an interrupted run can finish its draft;
it must never move a tag or overwrite a published package. An out-of-order or
conflicting version fails rather than replacing an existing release.

## Repository protections

The one-time repository setup follows `atrinik/classic`, adapted to this app:

- Main and `N.N.x` maintenance branches require pull requests, resolved threads,
  linear history and up-to-date passing checks. No second reviewer is required.
- A new maintenance branch can be created from an existing tag without requiring
  PR-only checks on that old commit. Subsequent updates require the checks.
- Required checks are **Velvet Scroll validation**, **CodeQL validation**, and
  **Conventional PR title**, produced by GitHub Actions.
- Force pushes and deletion are blocked for those branches and `v*` release tags.
- Repository admins have the reference setup's PR-only bypass for review/check
  and linear-history rules; branch/tag integrity has no bypass.
- Squash merging uses the PR title/body; merged feature branches are deleted.
- Actions have a read-only default token, cannot approve PRs, and allow GitHub
  actions plus the Rust toolchain action. Only the publisher gets contents write.
- Secret scanning, push protection, vulnerability alerts and
  Dependabot security fixes are enabled.

GitHub left secret validity checks disabled after the API update. To match that
remaining reference option, enable validity checks in Settings → Security →
Secret Protection if the option is available for this personal repository.

These are repository settings, not a workflow that repeatedly resets maintainer
choices. Review changes to workflow files and release guards carefully.
