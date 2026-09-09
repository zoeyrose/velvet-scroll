import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { downloadArtifact } from './download.mjs';
import { githubClient } from './github.mjs';

const bytes = Buffer.from('test artifact archive');
const plan = { repository: 'zoeyrose/velvet-scroll', artifactId: 30, artifactDigest: createHash('sha256').update(bytes).digest('hex') };
const redirect = async () => new Response(null, { status: 302, headers: { location: 'https://storage.example.com/signed-archive' } });

test('artifact download binds the exact ID, verifies its digest, and does not forward credentials', async () => {
  const calls = [];
  const downloaded = await downloadArtifact(plan, async (path, options) => {
    assert.equal(path, 'repos/zoeyrose/velvet-scroll/actions/artifacts/30/zip');
    assert.equal(options.raw, true);
    return redirect();
  }, async (url, options) => {
    calls.push({ url, options });
    return new Response(bytes);
  });
  assert.deepEqual(downloaded, bytes);
  assert.equal(calls[0].options.headers, undefined);
  assert.equal(calls[0].options.redirect, 'error');
});

test('corrupt, insecure, oversized, and failed downloads are rejected', async () => {
  await assert.rejects(downloadArtifact(plan, redirect, async () => new Response('corrupt')), /checksum/);
  await assert.rejects(downloadArtifact(plan, redirect, async () => new Response(null, { status: 403 })), /download failed/);
  await assert.rejects(downloadArtifact(plan, redirect, async () => new Response(bytes, { headers: { 'Content-Length': 60 * 1024 * 1024 } })), /size limit/);
  for (const location of ['http://storage.example.com/a', 'https://user:password@storage.example.com/a']) {
    await assert.rejects(downloadArtifact(plan, async () => new Response(null, { status: 302, headers: { location } })), /Invalid artifact download URL/);
  }
});

test('GitHub client keeps credentials on api.github.com and rejects redirects for JSON requests', async () => {
  const api = githubClient('test-token', async (url, options) => {
    assert.equal(url, 'https://api.github.com/repos/zoeyrose/velvet-scroll');
    assert.equal(options.headers.Authorization, 'Bearer test-token');
    assert.equal(options.redirect, 'manual');
    return new Response(null, { status: 302, headers: { location: 'https://elsewhere.example' } });
  });
  await assert.rejects(api('repos/zoeyrose/velvet-scroll'), /302/);
  await assert.rejects(api('https://evil.example'), /Invalid GitHub API path/);
});
