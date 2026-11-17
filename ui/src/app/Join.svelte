<script lang="ts">
  import { onMount } from 'svelte';

  import type { AgentStatus, Commands, LinkSummary } from '../lib/agent';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';

  let {
    banc,
    link,
    onJoined,
  }: { banc: Commands; link: string | null; onJoined: (status: AgentStatus) => void } = $props();

  let name = $state('');
  let server = $state('');
  let key = $state('');
  let summary = $state<LinkSummary | null>(null);
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    name = await banc.hostname().catch(() => '');
  });

  $effect(() => {
    summary = null;
    error = '';
    if (link) {
      banc.readLink(link).then(
        (value) => (summary = value),
        (e: unknown) => (error = errorMessage(e)),
      );
    }
  });

  async function join(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = '';
    try {
      const status =
        link && summary
          ? await banc.enrollWithLink(link, name)
          : await banc.enrollWithKey(server, key, name);
      onJoined(status);
    } catch (e) {
      error = errorMessage(e);
    }
    busy = false;
  }
</script>

<section>
  <h2>{t('join.title')}</h2>
  <form onsubmit={join}>
    {#if link && summary}
      <p>{t('join.from_link', { server: summary.server })}</p>
      <p><small>{t('join.fingerprint', { fingerprint: summary.fingerprint })}</small></p>
    {:else}
      <p>{t('join.intro')}</p>
      <label>
        {t('invites.server')}
        <input bind:value={server} required placeholder="https://banc.example.com" />
      </label>
      <label>{t('join.key')} <input bind:value={key} required autocomplete="off" /></label>
    {/if}
    <label>{t('join.name')} <input bind:value={name} required maxlength="64" /></label>
    {#if error}<p role="alert">{error}</p>{/if}
    <button type="submit" disabled={busy}>{t('join.submit')}</button>
  </form>
</section>
