<script lang="ts">
  import { isOlder, type AgentStatus, type Commands } from '../lib/agent';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';

  let {
    banc,
    status,
    appVersion,
    onChanged,
  }: {
    banc: Commands;
    status: AgentStatus;
    appVersion: string;
    onChanged: (next: AgentStatus) => void;
  } = $props();

  const update = $derived(status.update ?? null);
  // Installing by hand replaces the files, not the service already running.
  const outdated = $derived(appVersion !== '' && isOlder(status.version, appVersion));
  const working = $derived(
    update?.phase === 'checking' ||
      update?.phase === 'downloading' ||
      update?.phase === 'installing',
  );
  const message = $derived.by(() => {
    if (!update) {
      return t('update.unavailable');
    }
    if (update.phase === 'failed') {
      return update.version
        ? t('update.failed_version', { version: update.version })
        : t('update.failed');
    }
    return t(`update.${update.phase}`, { version: update.version ?? '' });
  });

  let busy = $state(false);
  let error = $state('');

  async function act(action: () => Promise<void>) {
    busy = true;
    error = '';
    try {
      await action();
    } catch (e) {
      error = errorMessage(e);
    }
    busy = false;
  }

  const check = () =>
    act(async () => {
      onChanged(await banc.checkUpdate());
    });

  const restart = () =>
    act(async () => {
      await banc.installAgent();
      onChanged(await banc.status());
    });
</script>

{#if error}<p role="alert">{error}</p>{/if}
{#if outdated}
  <p role="alert">
    {t('update.service_outdated', { running: status.version, installed: appVersion })}
  </p>
  <button onclick={restart} disabled={busy}>{t('update.restart')}</button>
{:else}
  <p class={update?.phase}>{message}</p>
  {#if update?.error}<p><small>{update.error}</small></p>{/if}
  {#if update}
    <button onclick={check} disabled={busy || working}>
      {update.phase === 'failed' ? t('update.retry') : t('update.check')}
    </button>
  {/if}
{/if}

<style>
  .up_to_date {
    color: #1b7a3a;
  }
  .failed {
    color: #b00020;
  }
</style>
