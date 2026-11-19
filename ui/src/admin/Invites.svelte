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

  function tone(status: InviteRow['status']): string {
    return status === 'pending' ? 'accent' : status === 'used' ? 'ok' : '';
  }

  async function copy(field: string, text: string) {
    await navigator.clipboard.writeText(text);
    copied = field;
  }
</script>

<section>
  <h2>{t('invites.new')}</h2>
  {#if canInvite}
    <form class="new" onsubmit={submit}>
      <label class="grow">
        {t('invites.user_name')}
        <input
          bind:value={userName}
          required
          autocomplete="off"
          placeholder={t('invites.user_placeholder')}
        />
      </label>
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
    <p class="banner warn">{t('invites.need_key')}</p>
  {/if}

  {#if created}
    <div class="created" aria-live="polite">
      <div class="created-head">
        <p>{t('invites.created', { name: created.invite.user_name })}</p>
        <button class="ghost small" onclick={() => (created = null)}>{t('common.close')}</button>
      </div>
      {#each [['link', created.link], ['key', created.key], ['server', created.server]] as const as [field, value] (field)}
        <div class="secret">
          <span class="secret-label">{t(`invites.${field}`)}</span>
          <code translate="no">{value}</code>
          <button class="small" class:primary={field === 'link'} onclick={() => copy(field, value)}>
            {copied === field ? t('common.copied') : t('common.copy')}
          </button>
        </div>
      {/each}
    </div>
  {/if}
</section>

<section class="flush">
  <h2>{t('invites.history')}</h2>
  {#if invites.length === 0}
    <p class="empty">{t('invites.empty')}</p>
  {:else}
    <div class="scroll">
      <table>
        <thead>
          <tr>
            <th>{t('invites.user_name')}</th>
            <th>{t('invites.person_role')}</th>
            <th>{t('invites.status')}</th>
            <th>{t('invites.created_at')}</th>
            <th><span class="sr">{t('invites.revoke')}</span></th>
          </tr>
        </thead>
        <tbody>
          {#each invites as invite (invite.id)}
            <tr>
              <td><strong>{invite.user_name}</strong></td>
              <td>
                {t(`person_role.${invite.person_role}`)}<br />
                <small>{t(`machine_role.${invite.machine_role}`)}</small>
              </td>
              <td>
                <span class="pill {tone(invite.status)}">
                  {t(`invite_status.${invite.status}`)}
                </span>
              </td>
              <td><small>{relativeTime(invite.created_at, now)}</small></td>
              <td class="actions">
                {#if invite.status === 'pending'}
                  <button class="small ghost danger" onclick={() => onRevoke(invite)}>
                    {t('invites.revoke')}
                  </button>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</section>

<style>
  .new {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 0.9rem;
    max-width: none;
  }
  .grow {
    flex: 1 1 14rem;
  }
  .new label:not(.grow) {
    flex: 0 1 12rem;
  }
  .banner {
    margin: 0;
  }
  .created {
    margin-top: 1.1rem;
    padding: 1rem;
    border-radius: var(--radius);
    background: var(--ok-soft);
    display: grid;
    gap: 0.6rem;
  }
  .created-head {
    display: flex;
    align-items: start;
    gap: 1rem;
  }
  .created-head p {
    flex: 1;
    margin: 0;
    font-weight: 550;
  }
  .secret {
    display: grid;
    grid-template-columns: 11rem minmax(0, 1fr) auto;
    align-items: center;
    gap: 0.75rem;
    padding: 0.5rem 0.6rem;
    background: var(--surface);
    border-radius: var(--radius-sm);
  }
  .secret-label {
    color: var(--text-2);
    font-size: 0.85rem;
  }
  @media (max-width: 40rem) {
    .secret {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .secret-label {
      grid-column: 1 / -1;
    }
  }
  .flush {
    padding: 1.1rem 0 0;
    overflow: hidden;
  }
  .flush h2 {
    padding: 0 1.25rem;
  }
  .scroll {
    overflow-x: auto;
  }
  .actions {
    text-align: right;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
</style>
