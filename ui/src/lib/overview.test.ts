import { describe, expect, it } from 'vitest';

import type { Machine, MachinePackages, ManifestView, PackageState } from './api';
import { activeMachines, attention, compliance } from './overview';

function machine(id: number, name: string, revoked = false): Machine {
  return {
    id,
    name,
    public_key: `key${id}`,
    os: 'macos',
    arch: 'aarch64',
    role: 'workstation',
    user_name: 'Camille',
    person_role: 'editor',
    enrolled_at: 0,
    last_seen_at: 0,
    agent_version: '0.1.0',
    os_version: null,
    hostname: null,
    resolve: null,
    revoked_at: revoked ? 1 : null,
    online: true,
  };
}

function line(
  id: number,
  version: number | null,
  states: Record<string, PackageState>,
  extra: Partial<MachinePackages['report']> = {},
): MachinePackages {
  return {
    machine_id: id,
    reported_at: 0,
    report: {
      manifest_version: version,
      refused: null,
      resolve_mismatch: false,
      resolve_required: null,
      packages: Object.entries(states).map(([pkg, state]) => ({
        id: pkg,
        version: null,
        state,
        error: state === 'failed' ? 'exit 1' : null,
        instructions: null,
      })),
      ...extra,
    },
  };
}

function view(published: number | null, changed = false): ManifestView {
  return {
    published:
      published === null
        ? null
        : {
            version: published,
            published_at: 0,
            manifest: { version: published, resolve: null, packages: [] },
          },
    draft: { resolve: null, packages: [] },
    changed,
    next_version: (published ?? 0) + 1,
  };
}

describe('compliance', () => {
  it('is unknown until the machine reports', () => {
    expect(compliance(undefined)).toBeNull();
  });

  it('leaves out the packages a machine cannot have', () => {
    const summary = compliance(
      line(1, 3, { a: 'installed', b: 'failed', c: 'unavailable', d: 'manual', e: 'pending' }),
    );
    expect(summary).toEqual({ total: 4, installed: 1, failed: 1, pending: 1, manual: 1 });
  });
});

describe('attention', () => {
  it('has nothing to say about a machine in step', () => {
    const items = attention([machine(1, 'A')], [line(1, 3, { a: 'installed' })], view(3), true);
    expect(items).toEqual([]);
  });

  it('asks for the admin key first', () => {
    const items = attention([], [], view(null, true), false);
    expect(items.map((item) => item.kind)).toEqual(['no_admin_key', 'unpublished']);
  });

  it('lists failures before manual steps', () => {
    const items = attention(
      [machine(1, 'A'), machine(2, 'B')],
      [line(1, 3, { a: 'manual' }), line(2, 3, { a: 'failed' })],
      view(3),
      true,
    );
    expect(items).toEqual([
      { kind: 'failed', severity: 'bad', machine: 'B', package: 'a', error: 'exit 1' },
      { kind: 'manual', severity: 'warn', machine: 'A', package: 'a' },
    ]);
  });

  it('reports a refused manifest and a wrong Resolve', () => {
    const items = attention(
      [machine(1, 'A')],
      [
        line(
          1,
          3,
          {},
          { refused: 'bad signature', resolve_mismatch: true, resolve_required: '20.2' },
        ),
      ],
      view(3),
      true,
    );
    expect(items.map((item) => item.kind)).toEqual(['refused', 'resolve_mismatch']);
  });

  it('says a machine is behind only when nothing worse is known', () => {
    const behind = attention([machine(1, 'A')], [line(1, 2, {})], view(3), true);
    expect(behind).toEqual([{ kind: 'behind', severity: 'warn', machine: 'A', version: 2 }]);
    const failed = attention([machine(1, 'A')], [line(1, 2, { a: 'failed' })], view(3), true);
    expect(failed.map((item) => item.kind)).toEqual(['failed']);
  });

  it('ignores revoked machines and machines that never reported', () => {
    const machines = [machine(1, 'A', true), machine(2, 'B')];
    expect(activeMachines(machines).map((item) => item.name)).toEqual(['B']);
    expect(attention(machines, [line(1, 1, { a: 'failed' })], view(3), true)).toEqual([]);
  });
});
