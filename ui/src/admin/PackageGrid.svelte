<script lang="ts">
  import type { Machine, MachinePackages, Manifest } from '../lib/api';
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
</script>

<section>
  <h2>{t('grid.title')}</h2>
  {#if !manifest}
    <p>{t('grid.empty')}</p>
  {:else}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th>{t('grid.machine')}</th>
            <th>{t('grid.manifest')}</th>
            {#each columns as column (column)}<th>{column}</th>{/each}
          </tr>
        </thead>
        <tbody>
          {#each active as machine (machine.id)}
            {@const line = lineOf(machine.id)}
            <tr>
              <td>
                {machine.name}
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
              <td>{line?.report.manifest_version ?? '–'}</td>
              {#each columns as column (column)}
                {@const cell = gridCell(line, column)}
                <td class={cell?.state ?? 'unknown'} title={cell?.error ?? ''}>
                  {cell ? t(`package_state.${cell.state}`) : t('package_state.unknown')}
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
  .scroll {
    overflow-x: auto;
  }
  .installed {
    color: #1b7a3a;
  }
  .failed {
    color: #b00020;
  }
  .pending,
  .manual,
  .unavailable {
    color: #8a5a00;
  }
  .unknown {
    color: #999;
  }
</style>
