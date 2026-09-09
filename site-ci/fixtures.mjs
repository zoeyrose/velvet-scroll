import assert from 'node:assert/strict';

export const SHA = 'a'.repeat(40);
export function fixture({ event = 'pull_request' } = {}) {
  const repository = 'zoeyrose/velvet-scroll';
  const prefix = `repos/${repository}`;
  const data = {
    [prefix]: { id: 10, full_name: repository, default_branch: 'main' },
    [`${prefix}/actions/runs/20`]: {
      id: 20, repository: { id: 10 }, name: 'Website checks', path: '.github/workflows/website-checks.yml',
      status: 'completed', conclusion: 'success', run_attempt: 2, head_sha: SHA,
      event, head_branch: event === 'push' ? 'main' : 'feat/site', head_repository: { id: 10 },
      triggering_actor: { login: 'builder' }, actor: { login: 'original-builder' }, pull_requests: [{ number: 7 }],
    },
    [`${prefix}/pulls/7`]: {
      state: 'open', merged: false, base: { ref: 'main', repo: { id: 10 } },
      head: { ref: 'feat/site', sha: SHA, repo: { id: 10 } }, user: { login: 'author' },
    },
    [`${prefix}/collaborators/author/permission`]: { permission: 'write' },
    [`${prefix}/collaborators/builder/permission`]: { permission: 'write', role_name: 'maintain' },
    [`${prefix}/git/ref/heads/main`]: { object: { type: 'commit', sha: SHA } },
    [`${prefix}/commits/${SHA}/pulls`]: [{ number: 7 }],
    [`${prefix}/actions/runs/20/artifacts`]: { artifacts: [{ id: 30, name: 'website-static-2', expired: false,
      workflow_run: { id: 20, head_sha: SHA }, size_in_bytes: 1024, digest: `sha256:${'b'.repeat(64)}` }] },
  };
  const calls = [];
  const api = async (path, options) => {
    calls.push({ path, options });
    assert.ok(Object.hasOwn(data, path), `Unexpected API request ${path}`);
    return structuredClone(data[path]);
  };
  return { repository, runId: 20, api, calls, data, prefix,
    run: data[`${prefix}/actions/runs/20`], pr: data[`${prefix}/pulls/7`],
    artifact: data[`${prefix}/actions/runs/20/artifacts`].artifacts[0] };
}
