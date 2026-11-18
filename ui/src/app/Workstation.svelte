<script lang="ts">
  import type { AgentStatus, Commands } from '../lib/agent';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';

  let {
    banc,
    status,
    onChanged,
  }: { banc: Commands; status: AgentStatus; onChanged: (next: AgentStatus) => void } = $props();

  const enrollment = $derived(status.enrollment);
  const packages = $derived(status.packages ?? null);
  let error = $state('');

  async function confirmManual(id: string) {
    error = '';
    try {
      onChanged(await banc.confirmManual(id));
    } catch (e) {
      error = errorMessage(e);
    }
  }
</script>

{#if enrollment}
  <section>
    <h2>{enrollment.name}</h2>
    <p class={enrollment.connection}>{t(`connection.${enrollment.connection}`)}</p>
    <dl>
      <dt>{t('workstation.server')}</dt>
      <dd>{enrollment.server}</dd>
      <dt>{t('invites.person_role')}</dt>
      <dd>
        {t(`person_role.${enrollment.person_role}`)}, {t(`machine_role.${enrollment.machine_role}`)}
      </dd>
      <dt>{t('machines.resolve')}</dt>
      <dd>
        {#if status.inventory.resolve}
          {status.inventory.resolve.version}
          {#if status.inventory.resolve.edition}({status.inventory.resolve.edition}){/if}
        {:else}
          {t('machines.resolve_missing')}
        {/if}
      </dd>
      <dt>{t('machines.system')}</dt>
      <dd>{status.inventory.os_version}</dd>
      <dt>{t('workstation.admin_key')}</dt>
      <dd><code>{enrollment.admin_fingerprint}</code></dd>
    </dl>
  </section>

  {#if packages}
    <section>
      <h2>{t('workstation.packages')}</h2>
      {#if error}<p role="alert">{error}</p>{/if}
      {#if packages.resolve_mismatch}
        <p role="alert">
          {t('workstation.resolve_mismatch', { version: packages.resolve_required ?? '' })}
        </p>
      {/if}
      {#if packages.refused}<p role="alert">{t('workstation.packages_refused')}</p>{/if}
      <ul>
        {#each packages.packages as item (item.id)}
          <li>
            <strong>{item.id}</strong>
            {#if item.version}{item.version}{/if}
            <span class={item.state}>{t(`package_state.${item.state}`)}</span>
            {#if item.error}<br /><small>{item.error}</small>{/if}
            {#if item.instructions}
              <p>{item.instructions}</p>
              <button onclick={() => confirmManual(item.id)}>
                {t('workstation.manual_done')}
              </button>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}
{/if}

<style>
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.4rem 1rem;
  }
  dt {
    color: #666;
  }
  dd {
    margin: 0;
  }
  ul {
    list-style: none;
    padding: 0;
  }
  li {
    padding: 0.35rem 0;
  }
  .installed,
  .connected {
    color: #1b7a3a;
  }
  .failed {
    color: #b00020;
  }
  .revoked {
    color: #b00020;
  }
</style>
