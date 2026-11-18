<script lang="ts">
  import { onMount } from 'svelte';

  import type { Commands } from '../lib/agent';
  import type { Manifest } from '../lib/api';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';
  import { fileSize } from '../lib/manifest';

  let { banc, server: initialServer }: { banc: Commands; server: string } = $props();

  // The enrolled server, once, as a starting value the admin can change.
  let server = $state('');
  let name = $state('');
  let password = $state('');
  let manifest = $state<Manifest | null>(null);
  let published = $state<number | null>(null);
  let error = $state('');
  let busy = $state(false);

  onMount(() => {
    server = initialServer;
  });

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

  const read = (event: SubmitEvent) => {
    event.preventDefault();
    return act(async () => {
      published = null;
      manifest = await banc.manifestPrepare(server, name, password);
      password = '';
    });
  };

  const sign = () =>
    act(async () => {
      published = await banc.manifestPublish();
      manifest = null;
    });

  const cancel = () =>
    act(async () => {
      await banc.manifestCancel();
      manifest = null;
    });
</script>

<h3>{t('publish.title')}</h3>
{#if error}<p role="alert">{error}</p>{/if}
{#if published !== null}<p>{t('publish.done', { version: published })}</p>{/if}

{#if manifest}
  <h4>{t('publish.review', { version: manifest.version })}</h4>
  <p>
    {manifest.resolve
      ? t('publish.resolve', { version: manifest.resolve.version })
      : t('publish.resolve_none')}
  </p>
  {#if manifest.packages.length === 0}<p role="alert">{t('publish.no_packages')}</p>{/if}
  <ul>
    {#each manifest.packages as item (item.id)}
      <li>
        <strong>{item.id}</strong>
        {item.version}
        {#if item.required}<small>{t('packages.required')}</small>{/if}
        <ul>
          {#each item.builds as build, index (index)}
            <li>
              {t(`os.${build.os}`)}
              {build.arch ?? t('packages.arch_any')}, {t(`package_kind.${build.kind}`)}
              {#if build.place}→ {t(`place.${build.place}`)}{/if}
              {#if build.file}
                <br />{build.file.name} ({fileSize(build.file.size)})
                <br /><small>{t('publish.hash')} <code>{build.file.blake3}</code></small>
              {/if}
              {#if build.args.length > 0}<br /><code>{build.args.join(' ')}</code>{/if}
              {#if build.instructions}<br />{build.instructions}{/if}
            </li>
          {/each}
        </ul>
      </li>
    {/each}
  </ul>
  <button onclick={sign} disabled={busy}>{t('publish.sign')}</button>
  <button onclick={cancel} disabled={busy}>{t('publish.cancel')}</button>
{:else}
  <p>{t('publish.intro')}</p>
  <form onsubmit={read}>
    <label>
      {t('invites.server')}
      <input bind:value={server} required placeholder="https://banc.example.com" />
    </label>
    <label>{t('login.name')} <input bind:value={name} required autocomplete="username" /></label>
    <label>
      {t('login.password')}
      <input type="password" bind:value={password} required autocomplete="current-password" />
    </label>
    <button type="submit" disabled={busy}>{t('publish.read')}</button>
  </form>
{/if}

<style>
  code {
    word-break: break-all;
  }
</style>
