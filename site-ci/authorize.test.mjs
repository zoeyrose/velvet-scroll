import assert from 'node:assert/strict';
import test from 'node:test';
import { authorizeDeployment } from './authorize.mjs';

import { fixture, SHA } from './fixtures.mjs';

test('current successful main builds authorize production, without trusting build metadata', async () => {
  const input = fixture({ event: 'push' });
  assert.deepEqual(await authorizeDeployment(input), {
    repository: input.repository, runId: 20, runAttempt: 2, sha: SHA,
    branch: 'main', environment: 'website-production', artifactId: 30, artifactDigest: 'b'.repeat(64),
  });
  assert.ok(input.calls.every(({ options }) => !options), 'authorization must be read-only');
});

test('current authorized PR builds use a fixed PR alias and bind the current run attempt', async () => {
  const input = fixture();
  const plan = await authorizeDeployment(input);
  assert.equal(plan.branch, 'pr-7');
  assert.equal(plan.environment, 'website-preview-7');
  assert.equal(plan.pullRequest, 7);
  assert.equal(plan.artifactId, 30);
  assert.ok(input.calls.some(({ path }) => path.includes('/collaborators/author/')));
  assert.ok(input.calls.some(({ path }) => path.includes('/collaborators/builder/')));
});

test('both author and triggering actor need write, maintain, or admin permission', async () => {
  for (const actor of ['author', 'builder']) {
    for (const permission of [{ permission: 'read' }, { permission: 'triage' }, {}, { permission: 'none' }]) {
      const input = fixture();
      input.data[`${input.prefix}/collaborators/${actor}/permission`] = permission;
      assert.equal(await authorizeDeployment(input), null);
      assert.ok(!input.calls.some(({ path }) => path.endsWith('/artifacts')));
    }
  }
  for (const role of ['write', 'maintain', 'admin']) {
    const input = fixture();
    input.data[`${input.prefix}/collaborators/author/permission`] = { role_name: role };
    assert.ok(await authorizeDeployment(input));
  }
});

test('forged commit authors and privileged original actors cannot authorize a rerun', async () => {
  const input = fixture();
  input.run.actor = { login: 'admin' };
  input.run.head_commit = { author: { email: 'owner@example.com' }, message: 'Authorized' };
  input.data[`${input.prefix}/collaborators/builder/permission`] = { permission: 'read' };
  assert.equal(await authorizeDeployment(input), null);
});

test('fork PRs require exact matching head-repository provenance and fresh association', async () => {
  const input = fixture();
  input.run.head_repository.id = 11;
  input.pr.head.repo.id = 11;
  input.run.pull_requests = [];
  assert.ok(await authorizeDeployment(input));
  input.pr.head.repo.id = 12;
  assert.equal(await authorizeDeployment(input), null);
});

test('stale main and PR commits, closed PRs, and wrong base branches skip deployment', async () => {
  const main = fixture({ event: 'push' });
  main.data[`${main.prefix}/git/ref/heads/main`].object.sha = 'c'.repeat(40);
  assert.equal(await authorizeDeployment(main), null);
  for (const mutate of [
    (input) => { input.pr.head.sha = 'c'.repeat(40); },
    (input) => { input.pr.state = 'closed'; },
    (input) => { input.pr.merged = true; },
    (input) => { input.pr.base.ref = '1.1.x'; },
    (input) => { input.pr.base.repo.id = 99; },
    (input) => { input.pr.head.ref = 'different'; },
    (input) => { input.run.pull_requests = [{ number: 7 }, { number: 8 }]; },
    (input) => { input.run.triggering_actor = undefined; },
  ]) {
    const input = fixture(); mutate(input);
    assert.equal(await authorizeDeployment(input), null);
  }
});

test('unsuccessful, unrelated, spoofed, or malformed workflow runs never deploy', async () => {
  for (const change of [
    { conclusion: 'failure' }, { status: 'in_progress' }, { event: 'pull_request_target' },
    { repository: { id: 99 } }, { name: 'Other checks' }, { path: '.github/workflows/evil.yml' },
    { head_sha: 'HEAD' }, { id: 21 }, { run_attempt: 0 },
  ]) {
    const input = fixture(); Object.assign(input.run, change);
    assert.equal(await authorizeDeployment(input), null);
  }
});

test('expired, oversized, duplicate, wrong-attempt or wrong-source artifacts are rejected', async () => {
  for (const change of [
    { expired: true }, { name: 'website-static-1' }, { workflow_run: { id: 99, head_sha: SHA } },
    { workflow_run: { id: 20, head_sha: 'c'.repeat(40) } }, { size_in_bytes: 51 * 1024 * 1024 },
    { size_in_bytes: -1 }, { digest: null }, { id: '30\nBAD=true' },
  ]) {
    const input = fixture(); Object.assign(input.artifact, change);
    assert.equal(await authorizeDeployment(input), null);
  }
  const input = fixture();
  input.data[`${input.prefix}/actions/runs/20/artifacts`].artifacts.push(structuredClone(input.artifact));
  assert.equal(await authorizeDeployment(input), null);
});

test('API errors fail closed and source identifiers cannot inject requests', async () => {
  const input = fixture();
  await assert.rejects(authorizeDeployment({ ...input, api: async () => { throw new Error('API unavailable'); } }), /API unavailable/);
  for (const repository of ['../evil', 'owner/repo?other=1', 'owner/repo\nBAD=true']) {
    await assert.rejects(authorizeDeployment({ ...input, repository }), /Invalid deployment source/);
  }
});
