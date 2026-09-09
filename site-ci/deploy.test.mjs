import assert from 'node:assert/strict';
import { existsSync } from 'node:fs';
import test from 'node:test';
import { authorizeDeployment } from './authorize.mjs';
import { deploy, deploymentArguments } from './deploy.mjs';
import { fixture, SHA } from './fixtures.mjs';

const env = {
  CLOUDFLARE_ACCOUNT_ID: 'd'.repeat(32), CLOUDFLARE_API_TOKEN: 'test-cloudflare-token',
  GITHUB_TOKEN: 'test-github-token', PATH: process.env.PATH, HOME: process.env.HOME,
  NODE_OPTIONS: '--require=untrusted.js', npm_config_registry: 'https://evil.example',
};

async function ready(options = {}) {
  const input = fixture(options);
  const plan = await authorizeDeployment(input);
  const mutations = [];
  const api = async (path, options) => {
    if (!options) return input.api(path);
    mutations.push({ path, ...options });
    return path.endsWith('/deployments') ? { id: 80 } : { id: 81 };
  };
  return { input, plan, mutations, api };
}

test('deploy reauthorizes, invokes local Wrangler without a shell, and publishes a GitHub deployment URL', async () => {
  const { plan, api, mutations } = await ready();
  let working;
  const url = await deploy({ plan, directory: '/tmp/validated-site', api, env, execute: (command, args, options) => {
    assert.match(command, /\/site-ci\/node_modules\/\.bin\/wrangler$/);
    assert.deepEqual(args, ['pages', 'deploy', '/tmp/validated-site', '--project-name', 'velvet-scroll',
      '--branch', 'pr-7', '--commit-hash', SHA, '--commit-message', `Website ${SHA}`, '--commit-dirty=false']);
    assert.equal(options.shell, undefined);
    assert.equal(options.env.GITHUB_TOKEN, undefined);
    assert.equal(options.env.NODE_OPTIONS, undefined);
    assert.equal(options.env.npm_config_registry, undefined);
    assert.equal(options.env.CLOUDFLARE_API_TOKEN, env.CLOUDFLARE_API_TOKEN);
    assert.match(options.env.PATH, /site-ci\/node_modules\/\.bin/);
    working = options.cwd;
    assert.ok(existsSync(working));
    assert.notEqual(working, '/tmp/validated-site');
    return 'Deployment complete: https://abc123.velvet-scroll.pages.dev\n';
  } });
  assert.equal(url, 'https://abc123.velvet-scroll.pages.dev');
  assert.equal(existsSync(working), false);
  assert.equal(mutations[0].body.ref, SHA);
  assert.equal(mutations[0].body.production_environment, false);
  assert.equal(mutations[0].body.transient_environment, true);
  assert.deepEqual(mutations.at(-1).body, { state: 'success', environment_url: url, auto_inactive: true });
});

test('production marks its GitHub deployment as production', async () => {
  const { plan, api, mutations } = await ready({ event: 'push' });
  await deploy({ plan, directory: '/tmp/validated-site', api, env, execute: () => 'https://abc123.velvet-scroll.pages.dev' });
  assert.equal(mutations[0].body.production_environment, true);
  assert.equal(mutations[0].body.transient_environment, false);
});

test('a changed PR head or revoked permission stops before any write or command', async () => {
  for (const mutate of [
    (input) => { input.pr.head.sha = 'c'.repeat(40); },
    (input) => { input.data[`${input.prefix}/collaborators/author/permission`] = { permission: 'read' }; },
    (input) => { input.artifact.id = 31; },
  ]) {
    const { input, plan, api, mutations } = await ready();
    mutate(input);
    await assert.rejects(deploy({ plan, directory: '/tmp/site', api, env, execute: () => assert.fail('must not execute') }), /authorization changed/);
    assert.deepEqual(mutations, []);
  }
});

test('missing Cloudflare configuration fails before any deployment record or upload', async () => {
  const { plan, api, mutations } = await ready();
  await assert.rejects(deploy({ plan, directory: '/tmp/site', api, env: {}, execute: () => assert.fail('must not execute') }), /Configure CLOUDFLARE/);
  assert.deepEqual(mutations, []);
});

test('upload failures set failed deployment status without exposing child process diagnostics', async () => {
  const { plan, api, mutations } = await ready();
  await assert.rejects(deploy({ plan, directory: '/tmp/site', api, env, execute: () => {
    throw new Error('test-cloudflare-token must never appear in the propagated error');
  } }), (error) => {
    assert.equal(error.message, 'Website deployment failed');
    assert.equal(error.cause, undefined);
    return true;
  });
  assert.equal(mutations.at(-1).body.state, 'failure');
});

test('deployment arguments reject forged branches and commit IDs', () => {
  for (const branch of ['main\nEVIL=true', '--other-project', 'feature/x', 'pr-0']) {
    assert.throws(() => deploymentArguments({ sha: SHA, branch }, '/tmp/site'), /Invalid deployment plan/);
  }
  assert.throws(() => deploymentArguments({ sha: 'HEAD', branch: 'main' }, '/tmp/site'), /Invalid deployment plan/);
});
