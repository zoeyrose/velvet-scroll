# Working on Velvet Scroll

- Read the README and relevant source before changing behavior. Check which tools
  exist locally; use workspace-relative paths and temporary directories. Do not
  assume a particular distro, home directory, desktop session, or Rust installer.
- Keep the app small: the Rust daemon handles input; the PyQt6 UI is an
  unprivileged client. Use bounded parallel work when authorized and useful.
- Only wheel scrolling should change. Preserve exact mouse identity, DPI/wheel
  metadata, pointer deltas and physical event timing. Composite gaming mice can
  also expose keys, LEDs and absolute axes; preserve their behavior too.
- Never capture a device before its virtual mirror and calibration are ready.
  Keep device preparation and GUI/IPC delays out of pointer forwarding. Ensure
  grabs are released on errors and shutdown; exclude our own virtual devices.
- Default tests must not grab real mice, change device permissions, install host
  rules, or modify the user's desktop settings. Use isolated runtime/config
  directories for daemon tests. If a sandbox blocks sockets, explain the needed
  test access rather than weakening tests or marking them passed.
- Run the checks relevant to the change: `cargo fmt --all -- --check`, strict
  Clippy, `cargo test`, the offscreen UI suite, and `npm ci && npm test` for release
  tooling. Validate package metadata and staged install/uninstall when packaging
  changes. Do not claim physical feel or desktop compatibility from unit tests.
- Preserve unrelated changes. Use conventional commits. Existing user permission
  to commit/push remains valid; otherwise finish reviewable local work first.
- Versions are automated: main advances minor; maintenance `N.N.x` advances patch.
  `!` and `BREAKING CHANGE` never authorize a major. Do not manually bump package
  versions, edit release guards, create major marker commits, dispatch a major
  release, or move release tags unless the maintainer explicitly requests it.
- Never publish credentials, personal absolute paths, device serials, input
  captures, or private environment/configuration. Document portable commands and
  actual verification limits instead of copying session logs.
