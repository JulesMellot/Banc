import { describe, expect, it } from 'vitest';

import type { MachinePackages, PackageBuild } from './api';
import {
  blankBuild,
  fileSize,
  gridCell,
  gridColumns,
  kindsFor,
  packageFromFile,
  packageId,
  parseArgs,
  tidyBuild,
} from './manifest';

const file = { blake3: 'ab'.repeat(32), size: 3, name: 'plugin.zip' };

describe('manifest editing', () => {
  it('offers the installers of each system', () => {
    expect(kindsFor('macos')).toContain('pkg');
    expect(kindsFor('macos')).not.toContain('msi');
    expect(kindsFor('windows')).toContain('exe');
    expect(kindsFor('linux')).toContain('deb');
  });

  it('keeps a place for a copy only', () => {
    const font = tidyBuild({ ...blankBuild(), kind: 'font', file });
    expect(font.place).toBeNull();
    expect(font.file).toEqual(file);
    expect(tidyBuild({ ...blankBuild(), place: null }).place).toBe('ofx');
  });

  it('a manual package has instructions and no file', () => {
    const manual = tidyBuild({ ...blankBuild(), kind: 'manual', file });
    expect(manual.file).toBeNull();
    expect(manual.instructions).toBe('');
  });

  it('keeps arguments for the installers that take them', () => {
    const exe: PackageBuild = { ...blankBuild('windows'), kind: 'exe', args: ['/S'] };
    expect(tidyBuild(exe).args).toEqual(['/S']);
    expect(tidyBuild({ ...exe, kind: 'msi' }).args).toEqual([]);
  });

  it('falls back when the system changes', () => {
    const moved = tidyBuild({ ...blankBuild('windows'), kind: 'msi', os: 'macos' });
    expect(moved.kind).toBe('copy');
    expect(moved.place).toBe('ofx');
  });

  it('reads arguments typed on one line', () => {
    expect(parseArgs('  /S   /NORESTART ')).toEqual(['/S', '/NORESTART']);
    expect(parseArgs('')).toEqual([]);
  });

  it('makes identifiers the server accepts', () => {
    expect(packageId('4_SGamut3CineSLog3_To_Cine+709.cube', [])).toBe(
      '4-sgamut3cineslog3-to-cine-709',
    );
    expect(packageId('Étalonnage été.cube', [])).toBe('etalonnage-ete');
    expect(packageId('___.cube', [])).toBe('paquet');
    expect(packageId('a.cube', ['a', 'a-2'])).toBe('a-3');
    expect(packageId(`${'x'.repeat(200)}.cube`, []).length).toBeLessThanOrEqual(64);
  });

  it('sends a LUT to the LUT folder of every system', () => {
    const item = packageFromFile({ ...file, name: 'Team.cube' }, []);
    expect(item.id).toBe('team');
    expect(item.required).toBe(true);
    expect(item.builds.map((build) => build.os)).toEqual(['macos', 'windows', 'linux']);
    for (const build of item.builds) {
      expect(build.kind).toBe('copy');
      expect(build.place).toBe('lut');
      expect(build.file?.name).toBe('Team.cube');
    }
  });

  it('recognises what a file is', () => {
    const place = (name: string) => packageFromFile({ ...file, name }, []).builds[0]?.place;
    expect(place('Glow.zip')).toBe('ofx');
    expect(place('Sony LUTs.zip')).toBe('lut');
    expect(place('Title.drfx')).toBe('fusion_templates');
    expect(place('tool.py')).toBe('scripts');
    const font = packageFromFile({ ...file, name: 'Inter.otf' }, []);
    expect(font.builds[0]?.kind).toBe('font');
    expect(font.builds[0]?.place).toBeNull();
  });

  it('keeps an installer on its own system', () => {
    const item = packageFromFile({ ...file, name: 'Plugin.pkg' }, []);
    expect(item.builds).toHaveLength(1);
    expect(item.builds[0]?.os).toBe('macos');
    expect(item.builds[0]?.kind).toBe('pkg');
  });

  it('writes sizes the French way', () => {
    expect(fileSize(512)).toBe('512 o');
    expect(fileSize(1536)).toBe('1,5 Ko');
    expect(fileSize(5 * 1024 * 1024 * 1024)).toBe('5,0 Go');
  });
});

describe('machines by packages', () => {
  const line: MachinePackages = {
    machine_id: 1,
    reported_at: 0,
    report: {
      manifest_version: 2,
      refused: null,
      resolve_mismatch: false,
      resolve_required: null,
      packages: [
        { id: 'font', version: '1', state: 'failed', error: 'disk full', instructions: null },
      ],
    },
  };

  it('has one column per package', () => {
    expect(gridColumns(null)).toEqual([]);
    const manifest = {
      version: 1,
      resolve: null,
      packages: [{ id: 'font', version: '1', required: true, builds: [] }],
    };
    expect(gridColumns(manifest)).toEqual(['font']);
  });

  it('shows what the machine reported', () => {
    expect(gridCell(line, 'font')).toEqual({ state: 'failed', error: 'disk full' });
    expect(gridCell(line, 'other')).toBeNull();
    expect(gridCell(undefined, 'font')).toBeNull();
  });
});
