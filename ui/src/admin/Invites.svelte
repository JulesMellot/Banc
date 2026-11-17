<script lang="ts">
  import type { InviteCreated, InviteRow, MachineRole, NewInvite, PersonRole } from '../lib/api';
  import { relativeTime } from '../lib/format';
  import { t } from '../lib/i18n';

  let {
    invites,
    now,
    canInvite,
    onCreate,
    onRevoke,
  }: {
    invites: InviteRow[];
    now: number;
    canInvite: boolean;
    onCreate: (invite: NewInvite) => Promise<InviteCreated | null>;
    onRevoke: (invite: InviteRow) => void;
  } = $props();

  const personRoles: PersonRole[] = ['editor', 'colorist', 'admin'];
  const machineRoles: MachineRole[] = ['workstation', 'depot', 'render'];

  let userName = $state('');
  let personRole = $state<PersonRole>('editor');
  let machineRole = $state<MachineRole>('workstation');
  let created = $state<InviteCreated | null>(null);
  let copied = $state('');
  let busy = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    created = await onCreate({
      user_name: userName,
      person_role: personRole,
      machine_role: machineRole,
    });
    if (created) {
      userName = '';
    }
    busy = false;
  }

  async function copy(field: string, text: string) {
    await navigator.clipboard.writeText(text);
    copied = field;
  }
</script>

<section>
  <h2>{t('invites.title')}</h2>
  {#if canInvite}
    <form onsubmit={submit}>
      <label>{t('invites.user_name')} <input bind:value={userName} required /></label>
      <label>
        {t('invites.person_role')}
        <select bind:value={personRole}>
          {#each personRoles as role (role)}
            <option value={role}>{t(`person_role.${role}`)}</option>
          {/each}
        </select>
      </label>
      <label>
        {t('invites.machine_role')}
        <select bind:value={machineRole}>
          {#each machineRoles as role (role)}
            <option value={role}>{t(`machine_role.${role}`)}</option>
          {/each}
        </select>
      </label>
      <button type="submit" disabled={busy}>{t('invites.submit')}</button>
    </form>
  {:else}
    <p>{t('invites.need_key')}</p>
  {/if}

  {#if created}
    <div class="created">
      <p>{t('invites.created', { name: created.invite.user_name })}</p>
      {#each [['link', created.link], ['key', created.key], ['server', created.server]] as const as [field, value] (field)}
        <p>
          {t(`invites.${field}`)}<br />
          <code>{value}</code>
          <button onclick={() => copy(field, value)}>
            {copied === field ? t('common.copied') : t('common.copy')}
          </button>
        </p>
      {/each}
    </div>
  {/if}

  {#if invites.length === 0}
    <p>{t('invites.empty')}</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>{t('invites.user_name')}</th>
          <th>{t('invites.person_role')}</th>
          <th>{t('invites.status')}</th>
          <th>{t('invites.created_at')}</th>
          <th></th>
        </tr>
      </thead>
      <tbody>
        {#each invites as invite (invite.id)}
          <tr>
            <td>{invite.user_name}</td>
            <td>
              {t(`person_role.${invite.person_role}`)}<br />
              <small>{t(`machine_role.${invite.machine_role}`)}</small>
            </td>
            <td>{t(`invite_status.${invite.status}`)}</td>
            <td>{relativeTime(invite.created_at, now)}</td>
            <td>
              {#if invite.status === 'pending'}
                <button onclick={() => onRevoke(invite)}>{t('invites.revoke')}</button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  form {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 1rem;
    margin-bottom: 1rem;
  }
  label {
    display: grid;
    gap: 0.2rem;
  }
  .created {
    background: #eef6ee;
    border-radius: 6px;
    padding: 0.5rem 1rem;
    margin-bottom: 1rem;
  }
  code {
    word-break: break-all;
  }
</style>
