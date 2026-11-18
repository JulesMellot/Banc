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
      saved = false;
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
      saved = false;
    }
  }

  async function upload(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    for (const file of Array.from(input.files ?? [])) {
      uploading = file.name;
      await attempt(async () => {
        addPackage(await api.uploadPackageFile(file));
      });
    }
    uploading = '';
    input.value = '';
    await onChanged();
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
      await onChanged();
      // The application takes over: it shows the draft and signs it.
      window.location.href = 'banc://publish';
    }
  }
</script>

<section>
  <h2>{t('packages.title')}</h2>
  <p>{t('packages.intro')}</p>
  <p>
    {#if view.published}
      {t('packages.published', { version: view.published.version })}
    {:else}
      {t('packages.none_published')}
    {/if}
    {#if view.changed}<strong>{t('packages.unpublished_changes')}</strong>{/if}
  </p>

  <h3>{t('packages.files')}</h3>
  {#if files.length === 0}
    <p>{t('packages.no_files')}</p>
  {:else}
    <ul>
      {#each files as file (file.blake3)}
        <li>
          {file.name} <small>{fileSize(file.size)}</small>
          {#if !usedBy(file)}
            <button type="button" onclick={() => addPackage(file)}>
              {t('packages.add_file')}
            </button>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  <label>
    {t('packages.upload')}
    <input type="file" multiple onchange={upload} disabled={uploading !== ''} />
  </label>
  {#if uploading}<p>{t('packages.uploading', { name: uploading })}</p>{/if}

  <form onsubmit={save}>
    <label>
      {t('packages.resolve_version')}
      <input bind:value={resolveVersion} placeholder="21.1" />
      <small>{t('packages.resolve_hint')}</small>
    </label>

    <h3>{t('packages.list')}</h3>
    {#if draft.packages.length === 0}<p>{t('packages.empty')}</p>{/if}

    {#each draft.packages as item, packageIndex (packageIndex)}
      <fieldset>
        <label>
          {t('packages.id')}
          <input bind:value={item.id} required pattern="[a-z0-9\-]+" maxlength="64" />
          <small>{t('packages.id_hint')}</small>
        </label>
        <label>
          {t('packages.version')}
          <input bind:value={item.version} required maxlength="64" />
        </label>
        <label class="inline">
          <input type="checkbox" bind:checked={item.required} />
          {t('packages.required')}
        </label>

        <h4>{t('packages.builds')}</h4>
        {#each item.builds as build, index (index)}
          <div class="build">
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
              <label>
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
              <label>
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
              <label>
                {t('packages.args')}
                <input
                  value={build.args.join(' ')}
                  onchange={(event) =>
                    changeBuild(item, index, { args: parseArgs(event.currentTarget.value) })}
                />
                <small>{t('packages.args_hint')}</small>
              </label>
            {/if}
            <button type="button" onclick={() => item.builds.splice(index, 1)}>
              {t('packages.remove_build')}
            </button>
          </div>
        {/each}
        <button type="button" onclick={() => item.builds.push(blankBuild())}>
          {t('packages.add_build')}
        </button>
        <button type="button" onclick={() => draft.packages.splice(packageIndex, 1)}>
          {t('packages.remove')}
        </button>
      </fieldset>
    {/each}

    <button type="button" onclick={() => draft.packages.push(blankPackage())}>
      {t('packages.add')}
    </button>
    <button type="submit">{t('packages.save')}</button>
    {#if saved}<p>{t('packages.saved')}</p>{/if}
  </form>
</section>

<style>
  fieldset {
    margin: 1rem 0;
    border: 1px solid #ddd;
    border-radius: 6px;
  }
  .build {
    display: flex;
    flex-wrap: wrap;
    gap: 0.6rem;
    align-items: end;
    padding: 0.6rem 0;
    border-top: 1px solid #eee;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }
</style>
