<script lang="ts">
  import { listen } from '@tauri-apps/api/event';
  import { onMount } from 'svelte';

  import { commands, CommandError, type AgentStatus } from '../lib/agent';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';
  import AdminKey from './AdminKey.svelte';
  import Install from './Install.svelte';
  import Join from './Join.svelte';
  import Publish from './Publish.svelte';
  import Workstation from './Workstation.svelte';

  const banc = commands();

  let status = $state<AgentStatus | null>(null);
  let agentMissing = $state(false);
  let link = $state<string | null>(null);
  let error = $state('');
  let showAdmin = $state(false);
  let appVersion = $state('');
  let holdsAdminKey = $state(false);
  let publishWake = $state(0);

  async function refresh() {
    error = '';
    try {
      status = await banc.status();
      agentMissing = false;
    } catch (e) {
      if (e instanceof CommandError && e.code === 'agent_unreachable') {
        agentMissing = true;
      } else {
        error = errorMessage(e);
      }
    }
  }

  async function lookForAdminKey() {
    holdsAdminKey = (await banc.adminKeyStatus().catch(() => null)) !== null;
  }

  function toggleAdmin() {
    showAdmin = !showAdmin;
    // The key may have just been created or restored.
    void lookForAdminKey();
  }

  onMount(() => {
    let stop: (() => void) | undefined;
    let stopPublish: (() => void) | undefined;
    void (async () => {
      link = await banc.pendingLink().catch(() => null);
      appVersion = await banc.appVersion().catch(() => '');
      stop = await listen<string>('banc-link', (event) => {
        link = event.payload;
        showAdmin = false;
      });
      stopPublish = await listen('banc-publish', () => {
        showAdmin = false;
        publishWake += 1;
      });
      await lookForAdminKey();
      await refresh();
    })();
    // The state shown follows the agent while the window is open.
    const timer = setInterval(refresh, 5000);
    return () => {
      clearInterval(timer);
      stop?.();
      stopPublish?.();
    };
  });
</script>

<main>
  <h1>{t('app.name')}</h1>
  {#if error}<p role="alert">{error}</p>{/if}

  {#if showAdmin}
    <AdminKey {banc} server={status?.enrollment?.server ?? ''} />
  {:else if agentMissing}
    <Install {banc} onInstalled={refresh} />
  {:else if status && !status.enrollment}
    <Join {banc} {link} onJoined={(next) => ((status = next), (link = null))} />
  {:else if status?.enrollment}
    {#if holdsAdminKey && status.enrollment.connection !== 'revoked'}
      <Publish {banc} server={status.enrollment.server} wake={publishWake} />
    {/if}
    <Workstation {banc} {status} {appVersion} onChanged={(next) => (status = next)} />
    {#if status.enrollment.connection === 'revoked'}
      <Join {banc} {link} onJoined={(next) => ((status = next), (link = null))} />
    {/if}
  {:else}
    <p>{t('common.loading')}</p>
  {/if}

  <footer>
    <button class="link" onclick={toggleAdmin}>
      {showAdmin ? t('window.back') : t('adminkey.title')}
    </button>
  </footer>
</main>

<style>
  main {
    max-width: 44rem;
    margin: 1.5rem auto;
    padding: 0 1rem;
  }
  h1 {
    font-size: 1.4rem;
  }
  footer {
    margin-top: 2rem;
    text-align: right;
  }
  .link {
    background: none;
    border: none;
    color: #555;
    text-decoration: underline;
    cursor: pointer;
  }
</style>
