<script lang="ts">
  import type { Machine, MachinePackages, Manifest, PackageState } from '../lib/api';
  import { t } from '../lib/i18n';
  import { gridCell, gridColumns } from '../lib/manifest';

  let {
    machines,
    grid,
    manifest,
  }: { machines: Machine[]; grid: MachinePackages[]; manifest: Manifest | null } = $props();

  const columns = $derived(gridColumns(manifest));
  const active = $derived(machines.filter((machine) => machine.revoked_at === null));
  const lineOf = (id: number) => grid.find((line) => line.machine_id === id);

  function tone(state: PackageState | undefined): string {
    switch (state) {
      case 'installed':
        return 'ok';
      case 'failed':
        return 'bad';
      case 'pending':
      case 'manual':
        return 'warn';
      default:
        return 'bare';
    }
  }
</script>

<section class="flush">
  <h2>{t('grid.title')}</h2>
  {#if !manifest}
    <p class="empty">{t('grid.empty')}</p>
  {:else}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th>{t('grid.machine')}</th>
            <th>{t('grid.manifest')}</th>
            {#each columns as column (column)}<th><code translate="no">{column}</code></th>{/each}
          </tr>
        </thead>
        <tbody>
          {#each active as machine (machine.id)}
            {@const line = lineOf(machine.id)}
            <tr>
              <td>
                <strong>{machine.name}</strong>
                {#if line?.report.resolve_mismatch}
                  <br /><small class="failed">
                    {t('grid.resolve_mismatch', {
                      version: line.report.resolve_required ?? '',
                    })}
                  </small>
                {/if}
                {#if line?.report.refused}
                  <br /><small class="failed">{t('grid.refused')}</small>
                {/if}
              </td>
              <td class="num">{line?.report.manifest_version ?? '–'}</td>
              {#each columns as column (column)}
                {@const cell = gridCell(line, column)}
                <td title={cell?.error ?? ''}>
                  {#if cell && cell.state !== 'unavailable'}
                    <span class="pill {tone(cell.state)}">{t(`package_state.${cell.state}`)}</span>
                  {:else}
                    <span class="none" title={t(`package_state.${cell?.state ?? 'unknown'}`)}>
                      <span aria-hidden="true">—</span>
                      <span class="sr">{t(`package_state.${cell?.state ?? 'unknown'}`)}</span>
                    </span>
                  {/if}
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .flush {
    padding: 1.1rem 0 0;
    overflow: hidden;
  }
  h2 {
    padding: 0 1.25rem;
  }
  .scroll {
    overflow-x: auto;
  }
  td {
    white-space: nowrap;
  }
  .num {
    font-variant-numeric: tabular-nums;
  }
  .failed {
    color: var(--bad);
  }
  .none {
    color: var(--text-3);
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>
