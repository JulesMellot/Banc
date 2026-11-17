import { describe, expect, it } from 'vitest';

import { ApiError, client } from './api';

function respond(status: number, body?: unknown): typeof fetch {
  return async () =>
    new Response(body === undefined ? null : JSON.stringify(body), {
      status,
      headers: { 'Content-Type': 'application/json' },
    });
}

describe('client', () => {
  it('sends JSON and returns the answer', async () => {
    let seen: RequestInit | undefined;
    const fetcher: typeof fetch = async (_input, init) => {
      seen = init;
      return new Response(JSON.stringify({ id: 1, name: 'jules' }), { status: 200 });
    };
    const admin = await client(fetcher).login('jules', 'secret password');
    expect(admin).toEqual({ id: 1, name: 'jules' });
    expect(seen?.method).toBe('POST');
    expect(seen?.headers).toEqual({ 'Content-Type': 'application/json' });
    expect(JSON.parse(String(seen?.body))).toEqual({ name: 'jules', password: 'secret password' });
  });

  it('returns nothing for 204', async () => {
    await expect(client(respond(204)).revokeMachine(3)).resolves.toBeUndefined();
  });

  it('turns an error answer into an ApiError with its code', async () => {
    const api = client(respond(403, { error: 'invite_used', message: 'invitation already used' }));
    const error = await api.machines().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).status).toBe(403);
    expect((error as ApiError).code).toBe('invite_used');
  });

  it('reports a server that does not answer', async () => {
    const fetcher: typeof fetch = async () => {
      throw new TypeError('Failed to fetch');
    };
    const error = await client(fetcher)
      .info()
      .catch((e: unknown) => e);
    expect((error as ApiError).code).toBe('network');
  });

  it('copes with an error answer that is not JSON', async () => {
    const fetcher: typeof fetch = async () => new Response('Bad Gateway', { status: 502 });
    const error = await client(fetcher)
      .info()
      .catch((e: unknown) => e);
    expect((error as ApiError).code).toBe('unknown');
  });
});
