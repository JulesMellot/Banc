<script lang="ts">
  import type { AgentStatus } from '../lib/agent';
  import { t } from '../lib/i18n';

  let { status }: { status: AgentStatus } = $props();

  const enrollment = $derived(status.enrollment);
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
  .connected {
    color: #1b7a3a;
  }
  .revoked {
    color: #b00020;
  }
</style>
