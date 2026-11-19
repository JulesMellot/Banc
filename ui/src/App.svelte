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
