<script lang="ts">
  import { onMount } from 'svelte';

  import {
    api,
    ApiError,
    type AdminKey,
    type InviteCreated,
    type InviteRow,
    type Machine,
    type NewInvite,
  } from '../lib/api';
  import { errorMessage, nowSeconds } from '../lib/format';
  import { t } from '../lib/i18n';
  import Invites from './Invites.svelte';
  import Machines from './Machines.svelte';

  let { onSessionExpired }: { onSessionExpired: () => void } = $props();

  let adminKey = $state<AdminKey | null>(null);
  let machines = $state<Machine[]>([]);
  let invites = $state<InviteRow[]>([]);
  let now = $state(nowSeconds());
  let error = $state('');

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
      [adminKey, machines, invites] = await Promise.all([
        api.adminKey(),
        api.machines(),
        api.invites(),
      ]);
      now = nowSeconds();
    });
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

  onMount(refresh);
</script>

{#if error}<p role="alert">{error}</p>{/if}

<section>
  <h2>{t('adminkey.title')}</h2>
  {#if adminKey?.fingerprint}
    <p>{t('adminkey.present', { fingerprint: adminKey.fingerprint })}</p>
  {:else if adminKey}
    <p role="alert">{t('adminkey.missing')}</p>
  {/if}
</section>

<Machines {machines} {now} onRevoke={revokeMachine} onRefresh={refresh} />

<Invites
  {invites}
  {now}
  canInvite={Boolean(adminKey?.fingerprint)}
  onCreate={createInvite}
  onRevoke={revokeInvite}
/>
