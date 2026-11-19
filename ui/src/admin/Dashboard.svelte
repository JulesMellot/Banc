<script lang="ts">
  import { onMount } from 'svelte';

  import {
    api,
    ApiError,
    type Admin,
    type AdminKey,
    type InviteCreated,
    type InviteRow,
    type Machine,
    type MachinePackages,
    type ManifestView,
    type NewInvite,
    type StoredFile,
  } from '../lib/api';
  import { errorMessage, nowSeconds } from '../lib/format';
  import { t } from '../lib/i18n';
  import { activeMachines, attention } from '../lib/overview';
  import Invites from './Invites.svelte';
  import Machines from './Machines.svelte';
  import Overview from './Overview.svelte';
  import PackageGrid from './PackageGrid.svelte';
  import Packages from './Packages.svelte';

  let {
    admin,
    onLogout,
    onSessionExpired,
  }: { admin: Admin; onLogout: () => void; onSessionExpired: () => void } = $props();

  type Page = 'overview' | 'machines' | 'packages' | 'invites';
  const PAGES: { page: Page; hash: string }[] = [
    { page: 'overview', hash: '#/' },
    { page: 'machines', hash: '#/postes' },
    { page: 'packages', hash: '#/paquets' },
    { page: 'invites', hash: '#/invitations' },
  ];

  const pageFromHash = (hash: string): Page =>
    PAGES.find((entry) => entry.hash === hash)?.page ?? 'overview';

  let page = $state<Page>(pageFromHash(window.location.hash));
  let adminKey = $state<AdminKey | null>(null);
  let machines = $state<Machine[]>([]);
  let invites = $state<InviteRow[]>([]);
  let manifest = $state<ManifestView | null>(null);
  let files = $state<StoredFile[]>([]);
  let grid = $state<MachinePackages[]>([]);
  let now = $state(nowSeconds());
  let loaded = $state(false);
  let error = $state('');

  const items = $derived(attention(machines, grid, manifest, Boolean(adminKey?.fingerprint)));
  const counts = $derived({
    overview: items.length,
    machines: activeMachines(machines).length,
    packages: manifest?.draft.packages.length ?? 0,
    invites: invites.filter((invite) => invite.status === 'pending').length,
  });

  /** Runs an action; a 401 sends back to the login form. */
  async function attempt(action: () => Promise<void>): Promise<boolean> {
    error = '';
    try {
      await action();
      return true;
    } catch (e) {
      if (e instanceof ApiError && e.status === 401) {
        onSessionExpired();
      } else {
        error = errorMessage(e);
      }
      return false;
    }
  }

  async function refresh() {
    await attempt(async () => {
      [adminKey, machines, invites, manifest, files, grid] = await Promise.all([
        api.adminKey(),
        api.machines(),
        api.invites(),
        api.manifest(),
        api.packageFiles(),
        api.packagesGrid(),
      ]);
      now = nowSeconds();
    });
    loaded = true;
  }

  async function createInvite(invite: NewInvite): Promise<InviteCreated | null> {
    let created: InviteCreated | null = null;
    const ok = await attempt(async () => {
      created = await api.createInvite(invite);
    });
    if (ok) {
      await refresh();
    }
    return created;
  }

  async function revokeInvite(invite: InviteRow) {
    if (window.confirm(t('invites.revoke_confirm', { name: invite.user_name }))) {
      await attempt(() => api.revokeInvite(invite.id));
      await refresh();
    }
  }

  async function revokeMachine(machine: Machine) {
    if (window.confirm(t('machines.revoke_confirm', { name: machine.name }))) {
      await attempt(() => api.revokeMachine(machine.id));
      await refresh();
    }
  }

  onMount(() => {
    const follow = () => (page = pageFromHash(window.location.hash));
    window.addEventListener('hashchange', follow);
    void refresh();
    return () => window.removeEventListener('hashchange', follow);
  });
</script>

<div class="shell">
  <aside>
    <div class="brand">
      <span class="logo" aria-hidden="true">B</span>
      <div>
        <strong>{t('app.name')}</strong>
        <small>{t('admin.title')}</small>
      </div>
    </div>
    <nav aria-label={t('nav.label')}>
      {#each PAGES as entry (entry.page)}
        <a href={entry.hash} aria-current={page === entry.page ? 'page' : undefined}>
          <span>{t(`nav.${entry.page}`)}</span>
          {#if loaded && counts[entry.page] > 0}
            <span class="count" class:alert={entry.page === 'overview'}>
              {counts[entry.page]}
            </span>
          {/if}
        </a>
      {/each}
    </nav>
    <div class="account">
      {#if adminKey?.fingerprint}
        <small class="key">
          {t('adminkey.title')}
          <code translate="no">{adminKey.fingerprint}</code>
        </small>
      {:else if adminKey}
        <small class="key missing">{t('admin.key_missing_short')}</small>
      {/if}
      <div class="who">
        <span class="avatar" aria-hidden="true">{admin.name.slice(0, 1).toUpperCase()}</span>
        <span class="name">{admin.name}</span>
      </div>
      <button class="ghost small logout" onclick={onLogout}>{t('admin.logout')}</button>
    </div>
  </aside>

  <main>
    <header>
      <h1>{t(`nav.${page}`)}</h1>
      <button class="ghost" onclick={refresh}>{t('common.refresh')}</button>
    </header>

    {#if error}<p class="banner" role="alert">{error}</p>{/if}

    {#if !loaded}
      <p class="muted" aria-live="polite">{t('common.loading')}</p>
    {:else if page === 'overview'}
      <Overview {machines} {grid} {manifest} {invites} {items} {now} />
    {:else if page === 'machines'}
      <Machines {machines} {grid} {now} onRevoke={revokeMachine} />
      <PackageGrid {machines} {grid} manifest={manifest?.published?.manifest ?? null} />
    {:else if page === 'packages'}
      {#if manifest}
        <Packages view={manifest} {files} {attempt} onChanged={refresh} />
      {/if}
    {:else}
      <Invites
        {invites}
        {now}
        canInvite={Boolean(adminKey?.fingerprint)}
        onCreate={createInvite}
        onRevoke={revokeInvite}
      />
    {/if}
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 15rem minmax(0, 1fr);
    min-height: 100vh;
  }
  aside {
    position: sticky;
    top: 0;
    height: 100vh;
    display: flex;
    flex-direction: column;
    gap: 1.25rem;
    padding: 1.1rem 0.75rem;
    background: var(--surface);
    border-right: 1px solid var(--border);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    padding: 0 0.5rem;
  }
  .brand div {
    display: grid;
    line-height: 1.2;
  }
  nav {
    display: grid;
    gap: 2px;
  }
  nav a {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 0.5rem;
    padding: 0.45rem 0.6rem;
    border-radius: var(--radius-sm);
    color: var(--text-2);
    text-decoration: none;
    font-weight: 550;
    white-space: nowrap;
  }
  nav a:hover {
    background: var(--sunken);
    color: var(--text);
  }
  nav a[aria-current='page'] {
    background: var(--accent-soft);
    color: var(--text);
  }
  .count {
    min-width: 1.4rem;
    padding: 0 0.4rem;
    border-radius: 999px;
    background: var(--sunken);
    color: var(--text-2);
    font-size: 0.75rem;
    font-weight: 650;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
  .count.alert {
    background: var(--accent);
    color: var(--accent-text);
  }
  .account {
    margin-top: auto;
    display: grid;
    gap: 0.6rem;
    padding: 0.75rem 0.5rem 0;
    border-top: 1px solid var(--border);
  }
  .key {
    display: grid;
    color: var(--text-3);
  }
  .key code {
    font-size: 0.95em;
  }
  .key.missing {
    color: var(--bad);
  }
  .who {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
  .logout {
    justify-self: start;
    padding-left: 0;
  }
  .avatar {
    display: grid;
    place-items: center;
    width: 1.75rem;
    height: 1.75rem;
    border-radius: 50%;
    background: var(--sunken);
    font-weight: 650;
    font-size: 0.8rem;
    flex: none;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 550;
  }
  main {
    padding: 1.5rem 2rem 3rem;
    max-width: 78rem;
    width: 100%;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
    margin-bottom: 1.25rem;
  }
  h1 {
    font-size: 1.4rem;
    font-weight: 700;
    margin: 0;
  }

  @media (max-width: 52rem) {
    .shell {
      grid-template-columns: minmax(0, 1fr);
    }
    aside {
      position: static;
      height: auto;
      flex-direction: row;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.75rem;
      padding: 0.75rem 1rem;
      border-right: none;
      border-bottom: 1px solid var(--border);
    }
    nav {
      order: 3;
      width: 100%;
      display: flex;
      overflow-x: auto;
    }
    .account {
      margin: 0 0 0 auto;
      padding: 0;
      border: none;
    }
    .key,
    .name {
      display: none;
    }
    .account {
      display: flex;
      align-items: center;
      gap: 0.25rem;
    }
    main {
      padding: 1.25rem 1rem 2rem;
    }
  }
</style>
