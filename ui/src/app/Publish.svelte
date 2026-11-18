<script lang="ts">
  import { onMount, untrack } from 'svelte';

  import type { Commands } from '../lib/agent';
  import type { Manifest } from '../lib/api';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';
  import { fileSize } from '../lib/manifest';

  /** How often an open window looks for a draft saved in the web admin. */
  const LOOK_EVERY_MS = 15000;

  let { banc, server, wake }: { banc: Commands; server: string; wake: number } = $props();

  let manifest = $state<Manifest | null>(null);
  let published = $state<number | null>(null);
  let postponed = $state(false);
  let error = $state('');
  let busy = $state(false);
  // One look at a time: what is signed must be the manifest on screen.
  let looking = false;

  /** Background looks stay silent: a server out of reach is shown elsewhere. */
  async function look(reportErrors: boolean) {
    if (looking || busy || manifest) {
      return;
    }
    looking = true;
    try {
      const proposal = await banc.manifestPrepare(server);
      manifest = proposal.changed && !postponed ? proposal.manifest : null;
      error = '';
    } catch (e) {
      if (reportErrors) {
        error = errorMessage(e);
      }
    }
    looking = false;
  }

  onMount(() => {
    const timer = setInterval(() => void look(false), LOOK_EVERY_MS);
    return () => clearInterval(timer);
  });

  // At the start, and each time the web admin asks for a signature.
  $effect(() => {
    if (wake >= 0) {
      untrack(() => {
        postponed = false;
        void look(true);
      });
    }
  });

  async function sign() {
    busy = true;
    error = '';
    try {
      published = await banc.manifestPublish();
    } catch (e) {
      error = errorMessage(e);
    }
    // After a refusal, the next look brings the current draft back to read.
    manifest = null;
    busy = false;
  }

  async function later() {
    postponed = true;
    manifest = null;
    await banc.manifestCancel().catch(() => undefined);
  }
</script>

{#if error}<p role="alert">{error}</p>{/if}
{#if published !== null}<p class="done">{t('publish.done', { version: published })}</p>{/if}

{#if manifest}
  <section>
    <h2>{t('publish.waiting')}</h2>
    <h3>{t('publish.review', { version: manifest.version })}</h3>
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
    <button onclick={later} disabled={busy}>{t('publish.later')}</button>
  </section>
{/if}

<style>
  code {
    word-break: break-all;
  }
  .done {
    color: #1b7a3a;
  }
</style>
