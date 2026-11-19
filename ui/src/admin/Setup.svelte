<script lang="ts">
  import { api, type Admin } from '../lib/api';
  import { errorMessage } from '../lib/format';
  import { t } from '../lib/i18n';

  let { onDone }: { onDone: (admin: Admin) => void } = $props();

  let token = $state('');
  let name = $state('');
  let password = $state('');
  let error = $state('');
  let busy = $state(false);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    busy = true;
    error = '';
    try {
      onDone(await api.setup(token, name, password));
    } catch (e) {
      error = errorMessage(e);
    }
    busy = false;
  }
</script>

<section>
  <h2>{t('setup.title')}</h2>
  <p>{t('setup.intro')}</p>
  <form onsubmit={submit}>
    <label
      >{t('setup.token')}
      <input bind:value={token} required autocomplete="off" spellcheck="false" /></label
    >
    <label
      >{t('login.name')}
      <input bind:value={name} required autocomplete="username" spellcheck="false" /></label
    >
    <label>
      {t('login.password')}
      <input
        type="password"
        bind:value={password}
        required
        minlength="12"
        autocomplete="new-password"
      />
      <small>{t('login.password_hint')}</small>
    </label>
    {#if error}<p role="alert">{error}</p>{/if}
    <button type="submit" disabled={busy}>{t('setup.submit')}</button>
  </form>
</section>
