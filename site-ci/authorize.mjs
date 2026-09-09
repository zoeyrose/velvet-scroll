import { appendFileSync, readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { githubClient } from './github.mjs';

export const ARTIFACT_LIMIT = 50 * 1024 * 1024;
const SHA = /^[a-f\d]{40}$/;
const positiveId = (value) => Number.isSafeInteger(value) && value > 0;
const loginPattern = /^[a-z\d][a-z\d_-]*(?:\[bot\])?$/i;

// All provenance and permission decisions use fresh GitHub API responses. Files
// and messages produced by the unprivileged build are never authorization data.
export async function authorizeDeployment({ runId, repository, api }) {
  if (!positiveId(runId) || !/^[a-z\d][a-z\d_-]*\/[a-z\d][a-z\d_.-]*$/i.test(repository ?? '')) {
    throw new Error('Invalid deployment source');
  }
  const prefix = `repos/${repository}`;
  const repo = await api(prefix);
  const run = await api(`${prefix}/actions/runs/${runId}`);
  if (repo.full_name !== repository || !positiveId(repo.id) || repo.default_branch !== 'main'
    || run.id !== runId || run.repository?.id !== repo.id
    || run.name !== 'Website checks' || run.path !== '.github/workflows/website-checks.yml'
    || run.status !== 'completed' || run.conclusion !== 'success'
    || !positiveId(run.run_attempt) || !SHA.test(run.head_sha ?? '')) return null;

  let branch;
  let environment;
  let pullRequest;
  if (run.event === 'push') {
    if (run.head_branch !== 'main' || run.head_repository?.id !== repo.id) return null;
    const main = await api(`${prefix}/git/ref/heads/main`);
    if (main.object?.type !== 'commit' || main.object.sha !== run.head_sha) return null;
    branch = 'main';
    environment = 'website-production';
  } else if (run.event === 'pull_request') {
    let candidates = run.pull_requests;
    // GitHub can omit pull_requests for fork runs. Resolve associations on the
    // server and still require one current PR matching the exact source below.
    if (!Array.isArray(candidates) || candidates.length === 0) {
      candidates = await api(`${prefix}/commits/${run.head_sha}/pulls`);
    }
    if (!Array.isArray(candidates) || candidates.length !== 1 || !positiveId(candidates[0].number)) return null;
    const pr = await api(`${prefix}/pulls/${candidates[0].number}`);
    if (pr.state !== 'open' || pr.merged || pr.base?.repo?.id !== repo.id || pr.base.ref !== 'main'
      || !positiveId(pr.head?.repo?.id) || pr.head.repo.id !== run.head_repository?.id
      || pr.head.sha !== run.head_sha || pr.head.ref !== run.head_branch) return null;
    const actors = new Set([pr.user?.login, run.triggering_actor?.login]);
    for (const actor of actors) {
      if (typeof actor !== 'string' || !loginPattern.test(actor)) return null;
      const permission = await api(`${prefix}/collaborators/${encodeURIComponent(actor)}/permission`);
      if (!['write', 'maintain', 'admin'].includes(permission.permission)
        && !['write', 'maintain', 'admin'].includes(permission.role_name)) return null;
    }
    pullRequest = candidates[0].number;
    branch = `pr-${pullRequest}`;
    environment = `website-preview-${pullRequest}`;
  } else return null;

  const listing = await api(`${prefix}/actions/runs/${runId}/artifacts`);
  const artifacts = listing.artifacts?.filter((artifact) => artifact.name === `website-static-${run.run_attempt}`);
  if (!Array.isArray(artifacts) || artifacts.length !== 1) return null;
  const artifact = artifacts[0];
  if (!positiveId(artifact.id) || artifact.expired || artifact.workflow_run?.id !== runId
    || artifact.workflow_run.head_sha !== run.head_sha
    || !Number.isSafeInteger(artifact.size_in_bytes) || artifact.size_in_bytes <= 0 || artifact.size_in_bytes > ARTIFACT_LIMIT
    || !/^sha256:[a-f\d]{64}$/.test(artifact.digest ?? '')) return null;
  return {
    repository, runId, runAttempt: run.run_attempt, sha: run.head_sha,
    branch, environment, ...(pullRequest ? { pullRequest } : {}),
    artifactId: artifact.id, artifactDigest: artifact.digest.slice(7),
  };
}

async function main() {
  const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, 'utf8'));
  if (process.env.GITHUB_EVENT_NAME !== 'workflow_run') throw new Error('Deployment requires a completed checks workflow');
  const plan = await authorizeDeployment({
    runId: event.workflow_run?.id,
    repository: process.env.GITHUB_REPOSITORY,
    api: githubClient(process.env.GITHUB_TOKEN),
  });
  if (plan) writeFileSync('site-deployment-plan.json', JSON.stringify(plan) + '\n');
  appendFileSync(process.env.GITHUB_OUTPUT, `authorized=${Boolean(plan)}\n`);
  console.log(plan ? `Authorized ${plan.environment} at ${plan.sha}` : 'Skipped: checks are stale, ineligible, or the preview is not authorized.');
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) await main();
