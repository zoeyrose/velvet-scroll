import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { ARTIFACT_LIMIT } from './authorize.mjs';
import { githubClient } from './github.mjs';

export async function downloadArtifact(plan, api, request = fetch) {
  const response = await api(`repos/${plan.repository}/actions/artifacts/${plan.artifactId}/zip`, { raw: true });
  if (response.status !== 302) throw new Error('Expected a GitHub artifact download redirect');
  const location = new URL(response.headers.get('location'));
  if (location.protocol !== 'https:' || location.username || location.password) throw new Error('Invalid artifact download URL');
  // The GitHub credential must never follow the signed storage redirect.
  const download = await request(location.href, { redirect: 'error', signal: AbortSignal.timeout(60_000) });
  if (!download.ok || !download.body || Number(download.headers.get('content-length')) > ARTIFACT_LIMIT) {
    throw new Error('Artifact download failed or exceeds the size limit');
  }
  const chunks = [];
  let size = 0;
  for await (const chunk of download.body) {
    size += chunk.byteLength;
    if (size > ARTIFACT_LIMIT) throw new Error('Artifact exceeds the size limit');
    chunks.push(chunk);
  }
  const archive = Buffer.concat(chunks);
  if (createHash('sha256').update(archive).digest('hex') !== plan.artifactDigest) throw new Error('Artifact checksum differs from GitHub metadata');
  return archive;
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const plan = JSON.parse(readFileSync('site-deployment-plan.json', 'utf8'));
  writeFileSync(process.argv[2], await downloadArtifact(plan, githubClient(process.env.GITHUB_TOKEN)));
}
