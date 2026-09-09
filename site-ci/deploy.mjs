import { execFileSync } from 'node:child_process';
import { appendFileSync, readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { authorizeDeployment } from './authorize.mjs';
import { githubClient } from './github.mjs';

const toolsDirectory = dirname(fileURLToPath(import.meta.url));

export function deploymentArguments(plan, directory) {
  if (!/^[a-f\d]{40}$/.test(plan.sha) || !/^(main|pr-[1-9]\d*)$/.test(plan.branch)) throw new Error('Invalid deployment plan');
  return ['pages', 'deploy', resolve(directory), '--project-name', 'velvet-scroll',
    '--branch', plan.branch, '--commit-hash', plan.sha, '--commit-message', `Website ${plan.sha}`, '--commit-dirty=false'];
}

export async function deploy({ plan, directory, api, execute = execFileSync, env = process.env }) {
  const current = await authorizeDeployment({ runId: plan.runId, repository: plan.repository, api });
  if (!current || JSON.stringify(current) !== JSON.stringify(plan)) throw new Error('Deployment authorization changed; rerun current checks');
  if (!/^[a-f\d]{32}$/i.test(env.CLOUDFLARE_ACCOUNT_ID ?? '') || !env.CLOUDFLARE_API_TOKEN) {
    throw new Error('Configure CLOUDFLARE_ACCOUNT_ID and CLOUDFLARE_API_TOKEN before deploying');
  }
  const deployment = await api(`repos/${plan.repository}/deployments`, {
    method: 'POST',
    body: { ref: plan.sha, auto_merge: false, required_contexts: [], environment: plan.environment,
      transient_environment: plan.branch !== 'main', production_environment: plan.branch === 'main',
      description: plan.branch === 'main' ? 'Velvet Scroll website' : `Website preview for PR ${plan.pullRequest}` },
  });
  if (!Number.isSafeInteger(deployment.id) || deployment.id <= 0) throw new Error('Invalid GitHub deployment response');
  const statusPath = `repos/${plan.repository}/deployments/${deployment.id}/statuses`;
  await api(statusPath, { method: 'POST', body: { state: 'in_progress' } });
  // Run Wrangler outside the checkout and extracted artifact. This prevents its
  // automatic discovery of repository config or Pages Functions, even if future
  // trusted build changes add such files beside the deployment tools.
  const working = mkdtempSync(join(tmpdir(), 'velvet-scroll-wrangler-'));
  try {
    const output = execute(join(toolsDirectory, 'node_modules/.bin/wrangler'), deploymentArguments(plan, directory), {
      cwd: working, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        PATH: `${join(toolsDirectory, 'node_modules/.bin')}:${env.PATH}`,
        HOME: env.HOME, CI: 'true', NO_COLOR: '1', WRANGLER_SEND_METRICS: 'false',
        CLOUDFLARE_ACCOUNT_ID: env.CLOUDFLARE_ACCOUNT_ID,
        CLOUDFLARE_API_TOKEN: env.CLOUDFLARE_API_TOKEN,
      },
    });
    const url = output.match(/https:\/\/[a-z\d-]+\.velvet-scroll\.pages\.dev\b/)?.[0];
    if (!url) throw new Error('Wrangler did not return a Velvet Scroll deployment URL');
    await api(statusPath, { method: 'POST', body: { state: 'success', environment_url: url, auto_inactive: true } });
    return url;
  } catch {
    await api(statusPath, { method: 'POST', body: { state: 'failure', description: 'Website deployment failed; see the Actions log.' } });
    // Do not dump child-process environment or raw stderr containing credentials.
    throw new Error('Website deployment failed');
  } finally {
    rmSync(working, { recursive: true, force: true });
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const plan = JSON.parse(readFileSync('site-deployment-plan.json', 'utf8'));
  const url = await deploy({ plan, directory: process.argv[2], api: githubClient(process.env.GITHUB_TOKEN) });
  console.log(`Deployed: ${url}`);
  appendFileSync(process.env.GITHUB_STEP_SUMMARY, `Website deployment: [${plan.environment}](${url})\n`);
}
