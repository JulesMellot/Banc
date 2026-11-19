<script lang="ts">
  import { t } from '../lib/i18n';
  import type { Compliance } from '../lib/overview';

  let { summary }: { summary: Compliance | null } = $props();

  const parts = $derived(
    summary
      ? [
          { key: 'ok', value: summary.installed },
          { key: 'warn', value: summary.pending + summary.manual },
          { key: 'bad', value: summary.failed },
        ].filter((part) => part.value > 0)
      : [],
  );
</script>

<span class="compliance">
  {#if !summary}
    <small>{t('machines.no_report')}</small>
  {:else if summary.total === 0}
    <small>{t('machines.compliance_none')}</small>
  {:else}
    <span class="bar" aria-hidden="true">
      {#each parts as part (part.key)}
        <span class={part.key} style:flex-grow={part.value}></span>
      {/each}
    </span>
    <small class:bad={summary.failed > 0}>
      {t('machines.compliance', { installed: summary.installed, total: summary.total })}
    </small>
  {/if}
</span>

<style>
  .compliance {
    display: inline-flex;
    align-items: center;
    gap: 0.5rem;
    min-width: 8.5rem;
    flex: none;
  }
  .bar {
    display: flex;
    gap: 2px;
    width: 4.5rem;
    height: 6px;
    border-radius: 999px;
    overflow: hidden;
  }
  .bar span {
    flex-basis: 0;
    border-radius: 999px;
  }
  .ok {
    background: var(--ok);
  }
  .warn {
    background: var(--warn);
  }
  .bad {
    background: var(--bad);
  }
  small {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  small.bad {
    background: none;
    color: var(--bad);
    font-weight: 600;
  }
</style>
