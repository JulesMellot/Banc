<script lang="ts">
  import {
    api,
    type ManifestDraft,
    type ManifestView,
    type Package,
    type PackageBuild,
    type StoredFile,
  } from '../lib/api';
  import { t } from '../lib/i18n';
  import {
    ARCHES,
    blankBuild,
    blankPackage,
    fileSize,
    kindsFor,
    packageFromFile,
    parseArgs,
    PLACES,
    SYSTEMS,
    takesArgs,
    tidyBuild,
  } from '../lib/manifest';

  let {
    view,
    files,
    attempt,
    onChanged,
  }: {
    view: ManifestView;
    files: StoredFile[];
    attempt: (action: () => Promise<void>) => Promise<boolean>;
    onChanged: () => Promise<void>;
  } = $props();

  // The admin's edits stay local until they are saved.
  let draft = $state<ManifestDraft>({ resolve: null, packages: [] });
  let loaded = $state(false);
  let resolveVersion = $state('');
  let uploading = $state('');
  let saved = $state(false);
  let dirty = $state(false);
  let dragging = $state(false);
  // Packages shown unfolded, by position. New ones open so they can be filled in.
  let unfolded = $state<number[]>([]);

  function toggle(index: number) {
    unfolded = unfolded.includes(index)
      ? unfolded.filter((other) => other !== index)
      : [...unfolded, index];
  }

  function touched() {
    dirty = true;
    saved = false;
  }

  function newPackage() {
    draft.packages.push(blankPackage());
    unfolded = [...unfolded, draft.packages.length - 1];
    touched();
  }

  function removePackage(index: number) {
    draft.packages.splice(index, 1);
    unfolded = unfolded
      .filter((other) => other !== index)
      .map((other) => (other > index ? other - 1 : other));
    touched();
  }

  const systemsOf = (item: Package) => [...new Set(item.builds.map((build) => build.os))];

  $effect(() => {
    if (!loaded) {
      draft = structuredClone($state.snapshot(view.draft));
      resolveVersion = view.draft.resolve?.version ?? '';
      loaded = true;
    }
  });

  function changeBuild(item: Package, index: number, change: Partial<PackageBuild>) {
    const current = item.builds[index];
    if (current) {
      item.builds[index] = tidyBuild({ ...current, ...change });
      touched();
    }
  }

  function chooseFile(item: Package, index: number, blake3: string) {
    const stored = files.find((file) => file.blake3 === blake3);
    const file = stored ? { blake3, size: stored.size, name: stored.name } : null;
    changeBuild(item, index, { file });
  }

  const usedBy = (file: StoredFile) =>
    draft.packages.some((item) => item.builds.some((build) => build.file?.blake3 === file.blake3));

  function addPackage(file: StoredFile) {
    if (!usedBy(file)) {
      const taken = draft.packages.map((item) => item.id);
      draft.packages.push(packageFromFile(file, taken));
      unfolded = [...unfolded, draft.packages.length - 1];
      touched();
    }
  }

  async function send(list: File[]) {
    for (const file of list) {
      uploading = file.name;
      await attempt(async () => {
        addPackage(await api.uploadPackageFile(file));
      });
    }
    uploading = '';
    await onChanged();
  }

  async function upload(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const list = Array.from(input.files ?? []);
    input.value = '';
    await send(list);
  }

  async function drop(event: DragEvent) {
    event.preventDefault();
    dragging = false;
    if (uploading === '') {
      await send(Array.from(event.dataTransfer?.files ?? []));
    }
  }

  async function save(event: SubmitEvent) {
    event.preventDefault();
    const version = resolveVersion.trim();
    const content: ManifestDraft = {
      resolve: version ? { version } : null,
      packages: $state.snapshot(draft.packages),
    };
    saved = await attempt(() => api.saveManifestDraft(content));
    if (saved) {
      dirty = false;
      await onChanged();
      // The application takes over: it shows the draft and signs it.
      window.location.href = 'banc://publish';
    }
  }
</script>

<div class="status">
  {#if view.published}
    <span class="pill ok"
      >{t('packages.status_published', { version: view.published.version })}</span
    >
  {:else}
    <span class="pill">{t('packages.none_published')}</span>
  {/if}
  {#if view.changed}<span class="pill accent">{t('packages.status_draft')}</span>{/if}
  <p class="muted">{t('packages.intro')}</p>
</div>

<form class="editor" onsubmit={save}>
  <section class="settings">
    <div>
      <h2>{t('packages.resolve_version')}</h2>
      <small>{t('packages.resolve_hint')}</small>
    </div>
    <label class="resolve">
      <span class="sr">{t('packages.resolve_version')}</span>
      <input
        bind:value={resolveVersion}
        oninput={touched}
        placeholder="21.1…"
        autocomplete="off"
        spellcheck="false"
      />
    </label>
  </section>

  <section class="list">
    <div class="head">
      <h2>{t('packages.list')}</h2>
      <button type="button" class="small" onclick={newPackage}>{t('packages.add')}</button>
    </div>
    {#if draft.packages.length === 0}<p class="empty">{t('packages.empty')}</p>{/if}

    {#each draft.packages as item, packageIndex (packageIndex)}
      {@const open = unfolded.includes(packageIndex)}
      <div class="package" class:open>
        <div class="summary">
          <button
            type="button"
            class="ghost fold"
            aria-expanded={open}
            onclick={() => toggle(packageIndex)}
          >
            <span class="chevron" aria-hidden="true">›</span>
            <code translate="no">{item.id || '…'}</code>
          </button>
          <span class="version">{item.version}</span>
          <span class="pill bare" class:accent={item.required}>
            {item.required ? t('packages.required_short') : t('packages.optional_short')}
          </span>
          <span class="systems">
            {#each systemsOf(item) as os (os)}<span class="chip">{t(`os.${os}`)}</span>{/each}
            {#if item.builds.length === 0}<small>{t('packages.no_builds')}</small>{/if}
          </span>
        </div>

        {#if open}
          <div class="body">
            <div class="fields">
              <label>
                {t('packages.id')}
                <input
                  bind:value={item.id}
                  oninput={touched}
                  required
                  pattern="[a-z0-9\-]+"
                  maxlength="64"
                  autocomplete="off"
                  spellcheck="false"
                />
                <small>{t('packages.id_hint')}</small>
              </label>
              <label>
                {t('packages.version')}
                <input
                  bind:value={item.version}
                  oninput={touched}
                  required
                  maxlength="64"
                  autocomplete="off"
                />
              </label>
              <label class="inline">
                <input type="checkbox" bind:checked={item.required} onchange={touched} />
                {t('packages.required')}
              </label>
            </div>

            <h3>{t('packages.builds')}</h3>
            {#each item.builds as build, index (index)}
              <div class="build">
                <div class="build-fields">
                  <label>
                    {t('packages.os')}
                    <select
                      value={build.os}
                      onchange={(event) =>
                        changeBuild(item, index, {
                          os: event.currentTarget.value as PackageBuild['os'],
                        })}
                    >
                      {#each SYSTEMS as os (os)}<option value={os}>{t(`os.${os}`)}</option>{/each}
                    </select>
                  </label>
                  <label>
                    {t('packages.arch')}
                    <select
                      value={build.arch ?? ''}
                      onchange={(event) =>
                        changeBuild(item, index, { arch: event.currentTarget.value || null })}
                    >
                      <option value="">{t('packages.arch_any')}</option>
                      {#each ARCHES as arch (arch)}<option value={arch}>{arch}</option>{/each}
                    </select>
                  </label>
                  <label>
                    {t('packages.kind')}
                    <select
                      value={build.kind}
                      onchange={(event) =>
                        changeBuild(item, index, {
                          kind: event.currentTarget.value as PackageBuild['kind'],
                        })}
                    >
                      {#each kindsFor(build.os) as kind (kind)}
                        <option value={kind}>{t(`package_kind.${kind}`)}</option>
                      {/each}
                    </select>
                  </label>
                  {#if build.kind === 'copy'}
                    <label>
                      {t('packages.place')}
                      <select
                        value={build.place}
                        onchange={(event) =>
                          changeBuild(item, index, {
                            place: event.currentTarget.value as PackageBuild['place'],
                          })}
                      >
                        {#each PLACES as place (place)}
                          <option value={place}>{t(`place.${place}`)}</option>
                        {/each}
                      </select>
                    </label>
                  {/if}
                  {#if build.kind === 'manual'}
                    <label class="wide">
                      {t('packages.instructions')}
                      <textarea
                        value={build.instructions ?? ''}
                        maxlength="300"
                        required
                        oninput={(event) =>
                          changeBuild(item, index, { instructions: event.currentTarget.value })}
                      ></textarea>
                    </label>
                  {:else}
                    <label class="wide">
                      {t('packages.file')}
                      <select
                        value={build.file?.blake3 ?? ''}
                        required
                        onchange={(event) => chooseFile(item, index, event.currentTarget.value)}
                      >
                        <option value="">{t('packages.file_choose')}</option>
                        {#each files as file (file.blake3)}
                          <option value={file.blake3}>{file.name}</option>
                        {/each}
                      </select>
                    </label>
                  {/if}
                  {#if takesArgs(build.kind)}
                    <label class="wide">
                      {t('packages.args')}
                      <input
                        value={build.args.join(' ')}
                        autocomplete="off"
                        spellcheck="false"
                        onchange={(event) =>
                          changeBuild(item, index, { args: parseArgs(event.currentTarget.value) })}
                      />
                      <small>{t('packages.args_hint')}</small>
                    </label>
                  {/if}
                </div>
                <button
                  type="button"
                  class="small ghost danger remove"
                  onclick={() => (item.builds.splice(index, 1), touched())}
                >
                  {t('packages.remove_build')}
                </button>
              </div>
            {/each}
            <div class="row">
              <button
                type="button"
                class="small"
                onclick={() => (item.builds.push(blankBuild()), touched())}
              >
                {t('packages.add_build')}
              </button>
              <button
                type="button"
                class="small ghost danger"
                onclick={() => removePackage(packageIndex)}
              >
                {t('packages.remove')}
              </button>
            </div>
          </div>
        {/if}
      </div>
    {/each}
  </section>

  <div class="publish" class:active={dirty || view.changed}>
    <p aria-live="polite">
      {#if saved}
        {t('packages.saved')}
      {:else if dirty}
        <strong>{t('packages.dirty')}</strong>
      {:else if view.changed}
        {t('packages.unpublished_changes')}
      {/if}
    </p>
    <button type="submit">{t('packages.save')}</button>
  </div>
</form>

<section>
  <h2>{t('packages.files')}</h2>
  <label
    class="drop"
    class:dragging
    ondragover={(event) => (event.preventDefault(), (dragging = true))}
    ondragleave={() => (dragging = false)}
    ondrop={drop}
  >
    <input type="file" multiple onchange={upload} disabled={uploading !== ''} />
    {#if uploading}
      <span aria-live="polite">{t('packages.uploading', { name: uploading })}</span>
    {:else}
      <strong>{t('packages.drop')}</strong>
      <small>{t('packages.drop_hint')}</small>
    {/if}
  </label>

  {#if files.length === 0}
    <p class="empty">{t('packages.no_files')}</p>
  {:else}
    <table>
      <tbody>
        {#each files as file (file.blake3)}
          <tr>
            <td class="file">{file.name}</td>
            <td class="size">{fileSize(file.size)}</td>
            <td class="actions">
              {#if usedBy(file)}
                <span class="pill ok">{t('packages.in_use')}</span>
              {:else}
                <button type="button" class="small" onclick={() => addPackage(file)}>
                  {t('packages.add_file')}
                </button>
              {/if}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</section>

<style>
  .status {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.5rem;
    margin-bottom: 1.25rem;
  }
  .status p {
    margin: 0 0 0 0.5rem;
    flex: 1;
    min-width: 16rem;
  }
  .editor {
    display: block;
    max-width: none;
  }
  .settings {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 1rem;
  }
  .settings h2 {
    margin-bottom: 0.15rem;
  }
  .resolve {
    width: 12rem;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.5rem;
  }
  .head h2 {
    margin: 0;
  }
  .list {
    padding-bottom: 0.5rem;
  }
  .package {
    border-top: 1px solid var(--border);
  }
  .summary {
    display: flex;
    align-items: center;
    gap: 0.75rem;
    flex-wrap: wrap;
    padding: 0.5rem 0;
  }
  .fold {
    padding-left: 0.25rem;
    color: var(--text);
    font-weight: 600;
  }
  .chevron {
    display: inline-block;
    width: 0.8rem;
    font-size: 1.1rem;
    line-height: 1;
    transition: transform 0.15s;
  }
  .open .chevron {
    transform: rotate(90deg);
  }
  .version {
    color: var(--text-2);
    font-variant-numeric: tabular-nums;
  }
  .systems {
    display: flex;
    gap: 0.35rem;
    margin-left: auto;
  }
  .chip {
    padding: 0.05rem 0.5rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    font-size: 0.78rem;
    color: var(--text-2);
  }
  .body {
    padding: 0.25rem 0 1rem 1.6rem;
  }
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(12rem, 1fr));
    gap: 0.9rem;
    align-items: start;
    margin-bottom: 1rem;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 0.5rem;
    align-self: center;
    font-weight: 450;
  }
  h3 {
    color: var(--text-2);
    font-size: 0.8rem;
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .build {
    display: flex;
    align-items: start;
    gap: 0.75rem;
    padding: 0.85rem;
    margin-bottom: 0.6rem;
    background: var(--surface-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }
  .build-fields {
    flex: 1;
    min-width: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10rem, 1fr));
    gap: 0.75rem;
    align-items: start;
  }
  .wide {
    grid-column: span 2;
  }
  .remove {
    flex: none;
    margin-top: 1.55rem;
  }
  .row {
    display: flex;
    gap: 0.5rem;
  }
  .publish {
    position: sticky;
    bottom: 1rem;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 1rem;
    padding: 0.7rem 0.75rem 0.7rem 1.1rem;
    margin-bottom: 1.25rem;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 6px 20px rgb(0 0 0 / 0.08);
  }
  .publish.active {
    border-color: var(--accent);
  }
  .publish p {
    flex: 1;
    margin: 0;
    color: var(--text-2);
  }
  .drop {
    position: relative;
    display: grid;
    justify-items: center;
    gap: 0.2rem;
    padding: 1.4rem 1rem;
    margin-bottom: 0.75rem;
    border: 1.5px dashed var(--border-strong);
    border-radius: var(--radius);
    background: var(--surface-2);
    color: var(--text-2);
    text-align: center;
    cursor: pointer;
    transition:
      border-color 0.12s,
      background-color 0.12s;
  }
  .drop:hover,
  .drop.dragging {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .drop:focus-within {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .drop input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }
  .drop strong {
    color: var(--text);
  }
  .file {
    word-break: break-all;
  }
  .size {
    color: var(--text-2);
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .actions {
    text-align: right;
    width: 1%;
    white-space: nowrap;
  }
  @media (prefers-reduced-motion: reduce) {
    .chevron {
      transition: none;
    }
  }
</style>
