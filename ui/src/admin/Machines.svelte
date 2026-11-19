<script lang="ts">
  import type { Machine, MachinePackages } from '../lib/api';
  import { relativeTime } from '../lib/format';
  import { t } from '../lib/i18n';
  import { compliance } from '../lib/overview';
  import ComplianceBar from './ComplianceBar.svelte';

  let {
    machines,
    grid,
    now,
    onRevoke,
  }: {
    machines: Machine[];
    grid: MachinePackages[];
    now: number;
    onRevoke: (machine: Machine) => void;
  } = $props();

  const active = $derived(machines.filter((machine) => machine.revoked_at === null));
  const revoked = $derived(machines.filter((machine) => machine.revoked_at !== null));
  const lineOf = (id: number) => grid.find((line) => line.machine_id === id);
</script>

{#snippet rows(list: Machine[], live: boolean)}
  <div class="scroll">
    <table>
      <thead>
        <tr>
          <th>{t('machines.name')}</th>
          <th>{t('machines.user')}</th>
          <th>{t('machines.system')}</th>
          <th>{t('machines.resolve')}</th>
          {#if live}<th>{t('machines.packages')}</th>{/if}
          <th>{t('machines.last_seen')}</th>
          <th><span class="sr">{t('machines.revoke')}</span></th>
        </tr>
      </thead>
      <tbody>
        {#each list as machine (machine.id)}
          <tr class:revoked={!live}>
            <td>
              <strong>{machine.name}</strong><br />
              <small>{t(`machine_role.${machine.role}`)}</small>
            </td>
            <td>
              {machine.user_name}<br />
              <small>{t(`person_role.${machine.person_role}`)}</small>
            </td>
            <td>
              {t(`os.${machine.os}`)}
              {machine.os_version ?? ''}<br />
              <small>{machine.arch} · Banc {machine.agent_version}</small>
            </td>
            <td>
              {#if machine.resolve}
                {machine.resolve.version}
                {#if machine.resolve.edition}<small>{machine.resolve.edition}</small>{/if}
              {:else}
                <small>{t('machines.resolve_missing')}</small>
              {/if}
            </td>
            {#if live}
              <td><ComplianceBar summary={compliance(lineOf(machine.id))} /></td>
            {/if}
            <td>
              {#if !live}
                <span class="pill">{t('machines.revoked')}</span>
              {:else if machine.online}
                <span class="pill ok">{t('machines.online')}</span>
              {:else}
                <span class="pill">{t('machines.offline')}</span>
              {/if}<br />
              <small>{relativeTime(machine.last_seen_at, now)}</small>
            </td>
            <td class="actions">
              {#if live}
                <button class="small ghost danger" onclick={() => onRevoke(machine)}>
                  {t('machines.revoke')}
                </button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/snippet}

<section class="flush">
  {#if active.length === 0}
    <p class="empty">{t('machines.empty')}</p>
  {:else}
    {@render rows(active, true)}
  {/if}
</section>

{#if revoked.length > 0}
  <details>
    <summary>{t('machines.revoked_list', { n: revoked.length })}</summary>
    <section class="flush">{@render rows(revoked, false)}</section>
  </details>
{/if}

<style>
  .flush {
    padding: 0;
    overflow: hidden;
  }
  .scroll {
    overflow-x: auto;
  }
  td {
    white-space: nowrap;
  }
  .actions {
    text-align: right;
  }
  .revoked td {
    color: var(--text-3);
  }
  summary {
    cursor: pointer;
    color: var(--text-2);
    font-weight: 550;
    margin-bottom: 0.75rem;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>
