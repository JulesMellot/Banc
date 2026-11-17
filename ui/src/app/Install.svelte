<script lang="ts">
  import type { Commands } from '../lib/agent';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';

  let { banc, onInstalled }: { banc: Commands; onInstalled: () => void } = $props();

  let busy = $state(false);
  let error = $state('');

  async function install() {
    busy = true;
    error = '';
    try {
      await banc.installAgent();
      onInstalled();
    } catch (e) {
      error = errorMessage(e);
    }
    busy = false;
  }
</script>

<section>
  <h2>{t('install.title')}</h2>
  <p>{t('install.intro')}</p>
  {#if error}<p role="alert">{error}</p>{/if}
  <button onclick={install} disabled={busy}>{t('install.submit')}</button>
</section>
