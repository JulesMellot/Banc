import { describe, expect, it } from 'vitest';

import { CommandError, commands, inWindow } from './agent';

describe('commands', () => {
  it('passes the arguments with the names the Rust side expects', async () => {
    const calls: [string, unknown][] = [];
    const client = commands(async <T>(command: string, args?: Record<string, unknown>) => {
      calls.push([command, args]);
      return undefined as T;
    });
    await client.enrollWithKey('https://banc.example.com', 'abc-def', 'Station 3');
    await client.adminKeyConfirm('7K3QD');
    expect(calls).toEqual([
      [
        'enroll',
        { link: null, server: 'https://banc.example.com', key: 'abc-def', name: 'Station 3' },
      ],
      ['admin_key_confirm', { lastGroup: '7K3QD' }],
    ]);
  });

  it('turns a refusal into a CommandError with its code', async () => {
    const client = commands(async () => {
      throw { code: 'agent_unreachable', message: 'no socket' };
    });
    const error = await client.status().catch((e: unknown) => e);
    expect(error).toBeInstanceOf(CommandError);
    expect((error as CommandError).code).toBe('agent_unreachable');
  });

  it('keeps a failure without a code readable', async () => {
    const client = commands(async () => {
      throw 'boom';
    });
    const error = (await client.status().catch((e: unknown) => e)) as CommandError;
    expect(error.code).toBe('unknown');
    expect(error.message).toBe('boom');
  });
});

describe('inWindow', () => {
  it('detects the Tauri window', () => {
    expect(inWindow({ __TAURI_INTERNALS__: {} })).toBe(true);
    expect(inWindow({})).toBe(false);
  });
});
