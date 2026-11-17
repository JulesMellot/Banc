<script lang="ts">
  import Admin from './admin/Admin.svelte';
</script>

<Admin />

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
</style>
