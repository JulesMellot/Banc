<script lang="ts">
  import Admin from './admin/Admin.svelte';
  import Window from './app/Window.svelte';
  import { inWindow } from './lib/agent';
</script>

{#if inWindow()}
  <Window />
{:else}
  <Admin />
{/if}

<style>
  :global(body) {
    font-family: system-ui, sans-serif;
    margin: 0;
    color: #1d1d1f;
    background: #f5f5f7;
  }
  :global(table) {
    border-collapse: collapse;
    width: 100%;
  }
  :global(th),
  :global(td) {
    text-align: left;
    padding: 0.4rem 0.6rem;
    border-bottom: 1px solid #ddd;
  }
  :global(section) {
    background: white;
    border-radius: 8px;
    padding: 1rem 1.5rem;
    margin-bottom: 1.5rem;
  }
  :global([role='alert']) {
    color: #b00020;
  }
  :global(form) {
    display: grid;
    gap: 0.8rem;
    max-width: 30rem;
  }
  :global(label) {
    display: grid;
    gap: 0.2rem;
  }
  :global(code) {
    word-break: break-all;
  }
</style>
