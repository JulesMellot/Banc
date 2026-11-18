// Editing the team's manifest. The rules mirror crates/banc-proto/src/manifest.rs:
// the server and the signing application check them again.
import type {
  MachinePackages,
  Manifest,
  Os,
  Package,
  PackageBuild,
  PackageFile,
  PackageKind,
  PackageState,
  Place,
} from './api';

export const SYSTEMS: Os[] = ['macos', 'windows', 'linux'];
export const ARCHES = ['x86_64', 'aarch64'];
export const PLACES: Place[] = ['ofx', 'lut', 'fusion_templates', 'scripts'];

const KINDS: Record<Os, PackageKind[]> = {
  macos: ['copy', 'font', 'pkg', 'manual'],
  windows: ['copy', 'font', 'msi', 'exe', 'manual'],
  linux: ['copy', 'font', 'deb', 'rpm', 'run', 'manual'],
};

/** The ways of installing a package on a system. */
export function kindsFor(os: Os): PackageKind[] {
  return KINDS[os];
}

export function takesArgs(kind: PackageKind): boolean {
  return kind === 'exe' || kind === 'run';
}

export function blankBuild(os: Os = 'macos'): PackageBuild {
  return {
    os,
    arch: null,
    kind: 'copy',
    file: null,
    place: 'ofx',
    args: [],
    instructions: null,
  };
}

export function blankPackage(): Package {
  return { id: '', version: '1.0', required: true, builds: [blankBuild()] };
}

const PLACE_OF_EXTENSION: Record<string, Place> = {
  cube: 'lut',
  '3dl': 'lut',
  dat: 'lut',
  ilut: 'lut',
  olut: 'lut',
  setting: 'fusion_templates',
  drfx: 'fusion_templates',
  py: 'scripts',
  lua: 'scripts',
};
const FONT_EXTENSIONS = ['ttf', 'otf', 'ttc'];
const INSTALLERS: Record<string, { os: Os; kind: PackageKind }> = {
  pkg: { os: 'macos', kind: 'pkg' },
  msi: { os: 'windows', kind: 'msi' },
  exe: { os: 'windows', kind: 'exe' },
  deb: { os: 'linux', kind: 'deb' },
  rpm: { os: 'linux', kind: 'rpm' },
  run: { os: 'linux', kind: 'run' },
};

/** An identifier for a file name, different from those already taken. */
export function packageId(name: string, taken: string[]): string {
  const base =
    name
      .replace(/\.[^.]*$/, '')
      .normalize('NFD')
      .replace(/[\u0300-\u036f]/g, '')
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 56)
      .replace(/-+$/, '') || 'paquet';
  let id = base;
  for (let number = 2; taken.includes(id); number += 1) {
    id = `${base}-${number}`;
  }
  return id;
}

/** A package ready to publish from an uploaded file: what the file is decides
 * where it goes. An archive of LUTs says so in its name, others are plugins. */
export function packageFromFile(stored: PackageFile, taken: string[]): Package {
  const extension = stored.name.split('.').pop()?.toLowerCase() ?? '';
  const file = { blake3: stored.blake3, size: stored.size, name: stored.name };
  const installer = INSTALLERS[extension];
  const builds = installer
    ? [{ ...blankBuild(installer.os), kind: installer.kind }]
    : SYSTEMS.map((os) => {
        if (FONT_EXTENSIONS.includes(extension)) {
          return { ...blankBuild(os), kind: 'font' as const };
        }
        const archiveOfLuts = extension === 'zip' && /lut/i.test(stored.name);
        const place = archiveOfLuts ? 'lut' : (PLACE_OF_EXTENSION[extension] ?? 'ofx');
        return { ...blankBuild(os), place };
      });
  return {
    id: packageId(stored.name, taken),
    version: '1.0',
    required: true,
    builds: builds.map((build) => tidyBuild({ ...build, file })),
  };
}

/** Drops what the build's system and kind do not use. */
export function tidyBuild(build: PackageBuild): PackageBuild {
  const kind = kindsFor(build.os).includes(build.kind) ? build.kind : 'copy';
  const manual = kind === 'manual';
  return {
    ...build,
    kind,
    file: manual ? null : build.file,
    place: kind === 'copy' ? (build.place ?? 'ofx') : null,
    args: takesArgs(kind) ? build.args : [],
    instructions: manual ? (build.instructions ?? '') : null,
  };
}

/** Arguments typed on one line. */
export function parseArgs(text: string): string[] {
  return text.split(/\s+/).filter((arg) => arg.length > 0);
}

/** A file size, in the units people use. */
export function fileSize(bytes: number): string {
  const units = ['o', 'Ko', 'Mo', 'Go', 'To'];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const rounded = unit === 0 ? String(value) : value.toFixed(1).replace('.', ',');
  return `${rounded} ${units[unit]}`;
}

/** The packages shown as columns of the grid. */
export function gridColumns(manifest: Manifest | null): string[] {
  return manifest ? manifest.packages.map((item) => item.id) : [];
}

/** What a machine says about a package. `null`: nothing to show. */
export function gridCell(
  line: MachinePackages | undefined,
  id: string,
): { state: PackageState; error: string | null } | null {
  const found = line?.report.packages.find((item) => item.id === id);
  return found ? { state: found.state, error: found.error } : null;
}
