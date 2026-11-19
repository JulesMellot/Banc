<script lang="ts">
  import { onMount } from 'svelte';

  import { api, ApiError, type Admin as AdminAccount, type Info } from '../lib/api';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';
  import Dashboard from './Dashboard.svelte';
  import Login from './Login.svelte';
  import Setup from './Setup.svelte';

  let info = $state<Info | null>(null);
  let admin = $state<AdminAccount | null>(null);
  let loading = $state(true);
  let error = $state('');

  async function load() {
    loading = true;
    error = '';
    try {
      info = await api.info();
      if (!info.setup_required) {
        admin = await api.me().catch((e: unknown) => {
          if (e instanceof ApiError && e.status === 401) {
            return null;
          }
          throw e;
        });
      }
    } catch (e) {
      error = errorMessage(e);
    }
    loading = false;
  }

  function signedIn(account: AdminAccount) {
    admin = account;
    if (info) {
      info.setup_required = false;
    }
  }

  async function logout() {
    await api.logout().catch(() => undefined);
    admin = null;
  }

  onMount(load);
</script>

{#if admin && !loading && !error}
  <Dashboard {admin} onLogout={logout} onSessionExpired={() => (admin = null)} />
{:else}
  <main class="auth">
    <div class="brand">
      <span class="logo" aria-hidden="true">B</span>
      <div>
        <strong>{t('app.name')}</strong>
        <small>{t('admin.title')}</small>
      </div>
    </div>
    {#if loading}
      <p class="muted" aria-live="polite">{t('common.loading')}</p>
    {:else if error}
      <section>
        <p role="alert">{error}</p>
        <button onclick={load}>{t('common.retry')}</button>
      </section>
    {:else if info?.setup_required}
      <Setup onDone={signedIn} />
    {:else}
      <Login onDone={signedIn} />
    {/if}
    <p class="tagline">{t('app.tagline')}</p>
  </main>
{/if}

<style>
  .auth {
    max-width: 26rem;
    margin: 0 auto;
    padding: 12vh 1rem 2rem;
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.7rem;
    margin-bottom: 1.5rem;
  }
  .brand div {
    display: grid;
    line-height: 1.2;
  }
  .brand strong {
    font-size: 1.15rem;
  }
  .tagline {
    margin-top: 1.5rem;
    color: var(--text-3);
    font-size: 0.85rem;
    text-align: center;
  }
  :global(.logo) {
    display: grid;
    place-items: center;
    width: 2rem;
    height: 2rem;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-text);
    font-weight: 750;
    font-size: 1rem;
    flex: none;
  }
  .auth :global(form) {
    max-width: none;
  }
  .auth :global(button[type='submit']) {
    width: 100%;
    margin-top: 0.25rem;
  }
</style>
