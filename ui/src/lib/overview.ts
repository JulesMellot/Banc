// What the admin overview puts in front of the admin: what needs a hand, and how far each
// machine is from the published manifest.
import type { Machine, MachinePackages, ManifestView } from './api';

export type Severity = 'bad' | 'warn';

export type Attention =
  | { kind: 'no_admin_key'; severity: Severity }
  | { kind: 'unpublished'; severity: Severity }
  | { kind: 'refused'; severity: Severity; machine: string }
  | { kind: 'resolve_mismatch'; severity: Severity; machine: string; version: string }
  | { kind: 'failed'; severity: Severity; machine: string; package: string; error: string | null }
  | { kind: 'manual'; severity: Severity; machine: string; package: string }
  | { kind: 'behind'; severity: Severity; machine: string; version: number | null };

/** How a machine stands against the packages it reported. */
export interface Compliance {
  /** Packages the machine must have: unavailable ones do not count. */
  total: number;
  installed: number;
  failed: number;
  pending: number;
  manual: number;
}

/** The machines that still count: revoked ones are left out. */
export function activeMachines(machines: Machine[]): Machine[] {
  return machines.filter((machine) => machine.revoked_at === null);
}

/** Sums up a machine's report. `null`: the machine has not reported yet. */
export function compliance(line: MachinePackages | undefined): Compliance | null {
  if (!line) {
    return null;
  }
  const summary: Compliance = { total: 0, installed: 0, failed: 0, pending: 0, manual: 0 };
  for (const item of line.report.packages) {
    if (item.state === 'unavailable') {
      continue;
    }
    summary.total += 1;
    summary[item.state] += 1;
  }
  return summary;
}

/**
 * Everything that needs the admin, worst first. A machine that is behind the published
 * manifest is only listed when nothing worse is said about it.
 */
export function attention(
  machines: Machine[],
  grid: MachinePackages[],
  manifest: ManifestView | null,
  hasAdminKey: boolean,
): Attention[] {
  const items: Attention[] = [];
  if (!hasAdminKey) {
    items.push({ kind: 'no_admin_key', severity: 'bad' });
  }
  const published = manifest?.published?.version ?? null;
  for (const machine of activeMachines(machines)) {
    const line = grid.find((entry) => entry.machine_id === machine.id);
    if (!line) {
      continue;
    }
    const report = line.report;
    const before = items.length;
    if (report.refused !== null) {
      items.push({ kind: 'refused', severity: 'bad', machine: machine.name });
    }
    if (report.resolve_mismatch) {
      items.push({
        kind: 'resolve_mismatch',
        severity: 'bad',
        machine: machine.name,
        version: report.resolve_required ?? '',
      });
    }
    for (const item of report.packages) {
      if (item.state === 'failed') {
        items.push({
          kind: 'failed',
          severity: 'bad',
          machine: machine.name,
          package: item.id,
          error: item.error,
        });
      } else if (item.state === 'manual') {
        items.push({ kind: 'manual', severity: 'warn', machine: machine.name, package: item.id });
      }
    }
    const behind =
      published !== null &&
      (report.manifest_version === null || report.manifest_version < published);
    if (behind && items.length === before) {
      items.push({
        kind: 'behind',
        severity: 'warn',
        machine: machine.name,
        version: report.manifest_version,
      });
    }
  }
  if (manifest?.changed) {
    items.push({ kind: 'unpublished', severity: 'warn' });
  }
  return items.sort((a, b) => rank(a.severity) - rank(b.severity));
}

function rank(severity: Severity): number {
  return severity === 'bad' ? 0 : 1;
}
