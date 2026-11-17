<script lang="ts">
  import type { Machine } from '../lib/api';
  import { relativeTime } from '../lib/format';
  import { t } from '../lib/i18n';

  let {
    machines,
    now,
    onRevoke,
    onRefresh,
  }: {
    machines: Machine[];
    now: number;
    onRevoke: (machine: Machine) => void;
    onRefresh: () => void;
  } = $props();
</script>

<section>
  <h2>{t('machines.title')} <button onclick={onRefresh}>{t('common.refresh')}</button></h2>
  {#if machines.length === 0}
    <p>{t('machines.empty')}</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>{t('machines.name')}</th>
          <th>{t('machines.user')}</th>
          <th>{t('machines.system')}</th>
          <th>{t('machines.resolve')}</th>
          <th>{t('machines.last_seen')}</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each machines as machine (machine.id)}
          <tr class:revoked={machine.revoked_at !== null}>
            <td>
              {machine.name}<br />
              <small>{t(`machine_role.${machine.role}`)}</small>
            </td>
            <td>
              {machine.user_name}<br />
              <small>{t(`person_role.${machine.person_role}`)}</small>
            </td>
            <td>
              {t(`os.${machine.os}`)}
              {machine.arch}<br />
              <small>{machine.os_version ?? ''}</small>
            </td>
            <td>
              {#if machine.resolve}
                {machine.resolve.version} ({machine.resolve.edition})
              {:else}
                {t('machines.resolve_missing')}
              {/if}
            </td>
            <td>
              {#if machine.revoked_at !== null}
                {t('machines.revoked')}
              {:else if machine.online}
                <strong>{t('machines.online')}</strong>
              {:else}
                {t('machines.offline')}
              {/if}<br />
              <small>{relativeTime(machine.last_seen_at, now)}</small>
            </td>
            <td>
              {#if machine.revoked_at === null}
                <button onclick={() => onRevoke(machine)}>{t('machines.revoke')}</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .revoked {
    color: #999;
  }
</style>
