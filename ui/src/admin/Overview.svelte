<script lang="ts">
  import type { InviteRow, Machine, MachinePackages, ManifestView } from '../lib/api';
  import { relativeTime } from '../lib/format';
  import { t } from '../lib/i18n';
  import { activeMachines, compliance, type Attention } from '../lib/overview';
  import ComplianceBar from './ComplianceBar.svelte';

  let {
    machines,
    grid,
    manifest,
    invites,
    items,
    now,
  }: {
    machines: Machine[];
    grid: MachinePackages[];
    manifest: ManifestView | null;
    invites: InviteRow[];
    items: Attention[];
    now: number;
  } = $props();

  const active = $derived(activeMachines(machines));
  const online = $derived(active.filter((machine) => machine.online).length);
  const pending = $derived(invites.filter((invite) => invite.status === 'pending').length);
  const lineOf = (id: number) => grid.find((line) => line.machine_id === id);

  function describe(item: Attention): string {
    switch (item.kind) {
      case 'no_admin_key':
      case 'unpublished':
        return t(`attention.${item.kind}`);
      case 'refused':
      case 'behind':
        return t(`attention.${item.kind}`, { machine: item.machine });
      case 'resolve_mismatch':
        return t('attention.resolve_mismatch', { machine: item.machine, version: item.version });
      case 'failed':
      case 'manual':
        return t(`attention.${item.kind}`, { machine: item.machine, package: item.package });
    }
  }

  function target(item: Attention): string {
    return item.kind === 'unpublished' ? '#/paquets' : '#/postes';
  }
</script>

<div class="tiles">
  <div class="tile">
    <span class="label">{t('overview.online')}</span>
    <span class="value">{t('overview.online_value', { online, total: active.length })}</span>
  </div>
  <div class="tile">
    <span class="label">{t('overview.attention')}</span>
    <span class="value" class:hot={items.some((item) => item.severity === 'bad')}>
      {items.length}
    </span>
  </div>
  <div class="tile">
    <span class="label">{t('overview.manifest')}</span>
    <span class="value">
      {manifest?.published
        ? t('overview.manifest_value', { version: manifest.published.version })
        : t('overview.manifest_none')}
    </span>
    {#if manifest?.changed}<span class="pill accent">{t('overview.manifest_draft')}</span>{/if}
  </div>
  <div class="tile">
    <span class="label">{t('overview.invites')}</span>
    <span class="value">{pending}</span>
  </div>
</div>

<div class="columns">
  <section>
    <h2>{t('overview.attention')}</h2>
    {#if items.length === 0}
      <p class="empty">{t('overview.all_good')}</p>
    {:else}
      <ul class="attention">
        {#each items as item, index (index)}
          <li>
            <span class="pill bare {item.severity}" aria-hidden="true">!</span>
            <span class="text">
              {describe(item)}
              {#if item.kind === 'failed' && item.error}
                <small><code>{item.error}</code></small>
              {/if}
            </span>
            {#if item.kind !== 'no_admin_key'}
              <a class="open" href={target(item)}>{t('attention.open')}</a>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <section>
    <div class="head">
      <h2>{t('nav.machines')}</h2>
      <a href="#/postes">{t('overview.see_all')}</a>
    </div>
    {#if active.length === 0}
      <p class="empty">{t('machines.empty')}</p>
    {:else}
      <ul class="machines">
        {#each active as machine (machine.id)}
          {@const summary = compliance(lineOf(machine.id))}
          <li>
            <span class="dot" class:on={machine.online}></span>
            <span class="who">
              <strong>{machine.name}</strong>
              <small>
                {machine.user_name} · {machine.online
                  ? t('machines.online')
                  : relativeTime(machine.last_seen_at, now)}
              </small>
            </span>
            <ComplianceBar {summary} />
          </li>
        {/each}
      </ul>
    {/if}
  </section>
</div>

<style>
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 9.5rem), 1fr));
    gap: 1rem;
    margin-bottom: 1.25rem;
  }
  .tile {
    display: grid;
    align-content: start;
    justify-items: start;
    gap: 0.2rem;
    padding: 1rem 1.1rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }
  .label {
    color: var(--text-2);
    font-size: 0.85rem;
  }
  .value {
    font-size: 1.5rem;
    font-weight: 650;
    font-variant-numeric: tabular-nums;
    line-height: 1.3;
  }
  .tile .pill {
    white-space: normal;
  }
  .value.hot {
    color: var(--bad);
  }
  .columns {
    display: grid;
    grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
    gap: 1.25rem;
    align-items: start;
  }
  @media (max-width: 64rem) {
    .columns {
      grid-template-columns: minmax(0, 1fr);
    }
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }
  a {
    color: var(--accent);
    font-weight: 550;
    text-decoration: none;
  }
  a:hover {
    text-decoration: underline;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    padding: 0.65rem 0;
    border-top: 1px solid var(--border);
  }
  li:first-child {
    border-top: none;
  }
  .attention .pill {
    width: 1.4rem;
    height: 1.4rem;
    padding: 0;
    justify-content: center;
    flex: none;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  .open {
    flex: none;
    font-size: 0.85rem;
  }
  .dot {
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
    background: var(--border-strong);
    flex: none;
  }
  .dot.on {
    background: var(--ok);
  }
  .who {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  .who strong,
  .who small {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
