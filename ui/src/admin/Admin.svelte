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

<header>
  <h1>{t('app.name')} <small>{t('admin.title')}</small></h1>
  {#if admin}
    <span>{admin.name}</span>
    <button onclick={logout}>{t('admin.logout')}</button>
  {/if}
</header>

<main>
  {#if loading}
    <p>{t('common.loading')}</p>
  {:else if error}
    <p role="alert">{error}</p>
    <button onclick={load}>{t('common.retry')}</button>
  {:else if info?.setup_required}
    <Setup onDone={signedIn} />
  {:else if !admin}
    <Login onDone={signedIn} />
  {:else}
    <Dashboard onSessionExpired={() => (admin = null)} />
  {/if}
</main>

<style>
  header {
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0 2rem;
    background: white;
    border-bottom: 1px solid #ddd;
  }
  header h1 {
    flex: 1;
    font-size: 1.3rem;
  }
  small {
    font-weight: normal;
    color: #666;
  }
  main {
    max-width: 70rem;
    margin: 2rem auto;
    padding: 0 1rem;
  }
</style>
