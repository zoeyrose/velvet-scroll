export function githubClient(token, request = fetch) {
  if (!token) throw new Error('GitHub token is missing');
  return async (path, { method = 'GET', body, raw = false } = {}) => {
    if (!path.startsWith('repos/') || /[\r\n?#]/.test(path)) throw new Error('Invalid GitHub API path');
    const response = await request(`https://api.github.com/${path}`, {
      method,
      headers: {
        Authorization: `Bearer ${token}`,
        Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28',
        ...(body ? { 'Content-Type': 'application/json' } : {}),
      },
      ...(body ? { body: JSON.stringify(body) } : {}),
      redirect: 'manual',
      signal: AbortSignal.timeout(30_000),
    });
    if (raw) return response;
    if (!response.ok) throw new Error(`GitHub API request failed (${response.status})`);
    return response.json();
  };
}
