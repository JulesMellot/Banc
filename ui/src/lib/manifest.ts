// Editing the team's manifest. The rules mirror crates/banc-proto/src/manifest.rs:
// the server and the signing application check them again.
import type {
  MachinePackages,
  Manifest,
  Os,
  Package,
  PackageBuild,
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
