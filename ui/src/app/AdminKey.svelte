<script lang="ts">
  import { onMount } from 'svelte';

  import type { Commands, KeyInfo, NewKey } from '../lib/agent';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';

  let { banc, server: initialServer }: { banc: Commands; server: string } = $props();

  let held = $state<KeyInfo | null>(null);
  let loaded = $state(false);
  let created = $state<NewKey | null>(null);
  let lastGroup = $state('');
  let restoring = $state(false);
  let recoveryCode = $state('');
  // The enrolled server, once, as a starting value the admin can change.
  let server = $state('');
  let name = $state('');
  let password = $state('');
  let replace = $state(false);
  let registered = $state(false);
  let error = $state('');
  let busy = $state(false);

  onMount(async () => {
    server = initialServer;
    try {
      held = await banc.adminKeyStatus();
    } catch (e) {
      error = errorMessage(e);
    }
    loaded = true;
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

  const generate = () =>
    act(async () => {
      created = await banc.adminKeyGenerate();
    });

  const confirm = (event: SubmitEvent) => {
    event.preventDefault();
    return act(async () => {
      held = await banc.adminKeyConfirm(lastGroup);
      created = null;
    });
  };

  const restore = (event: SubmitEvent) => {
    event.preventDefault();
    return act(async () => {
      held = await banc.adminKeyRestore(recoveryCode);
      restoring = false;
      recoveryCode = '';
    });
  };

  const register = (event: SubmitEvent) => {
    event.preventDefault();
    return act(async () => {
      await banc.adminKeyRegister(server, name, password, replace);
      password = '';
      registered = true;
    });
  };
</script>

<section>
  <h2>{t('adminkey.title')}</h2>
  {#if error}<p role="alert">{error}</p>{/if}

  {#if !loaded}
    <p>{t('common.loading')}</p>
  {:else if created}
    <p>{t('adminkey.write_down')}</p>
    <p class="code"><code>{created.recovery_code}</code></p>
    <form onsubmit={confirm}>
      <label>
        {t('adminkey.confirm_label')}
        <input bind:value={lastGroup} required autocomplete="off" maxlength="5" />
      </label>
      <button type="submit" disabled={busy}>{t('adminkey.confirm')}</button>
    </form>
  {:else if held}
    <p>{t('adminkey.held', { fingerprint: held.fingerprint })}</p>
    <h3>{t('adminkey.register_title')}</h3>
    <p>{t('adminkey.register_intro')}</p>
    <form onsubmit={register}>
      <label>
        {t('invites.server')}
        <input bind:value={server} required placeholder="https://banc.example.com" />
      </label>
      <label>{t('login.name')} <input bind:value={name} required autocomplete="username" /></label>
      <label>
        {t('login.password')}
        <input type="password" bind:value={password} required autocomplete="current-password" />
      </label>
      <label class="inline">
        <input type="checkbox" bind:checked={replace} />
        {t('adminkey.replace')}
      </label>
      {#if registered}<p>{t('adminkey.registered')}</p>{/if}
      <button type="submit" disabled={busy}>{t('adminkey.register')}</button>
    </form>
  {:else if restoring}
    <form onsubmit={restore}>
      <label>
        {t('adminkey.recovery_code')}
        <textarea bind:value={recoveryCode} required rows="3"></textarea>
      </label>
      <button type="submit" disabled={busy}>{t('adminkey.restore')}</button>
    </form>
  {:else}
    <p>{t('adminkey.none_here')}</p>
    <button onclick={generate} disabled={busy}>{t('adminkey.create')}</button>
    <button onclick={() => (restoring = true)} disabled={busy}>{t('adminkey.restore')}</button>
  {/if}
</section>

<style>
  .code {
    font-size: 1.2rem;
    background: #f0f0f3;
    padding: 0.8rem;
    border-radius: 6px;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
</style>
