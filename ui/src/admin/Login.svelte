<script lang="ts">
  import { api, type Admin } from '../lib/api';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';

  let { onDone }: { onDone: (admin: Admin) => void } = $props();

  let name = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = '';
    try {
      onDone(await api.login(name, password));
    } catch (e) {
      error = errorMessage(e);
    }
    busy = false;
  }
</script>

<section>
  <h2>{t('login.title')}</h2>
  <form onsubmit={submit}>
    <label
      >{t('login.name')}
      <input bind:value={name} required autocomplete="username" spellcheck="false" /></label
    >
    <label>
      {t('login.password')}
      <input type="password" bind:value={password} required autocomplete="current-password" />
    </label>
    {#if error}<p role="alert">{error}</p>{/if}
    <button type="submit" disabled={busy}>{t('login.submit')}</button>
  </form>
</section>
