<script lang="ts">
  import { store } from "../lib/store.svelte";
</script>

<div class="toasts">
  {#each store.toasts as t (t.id)}
    <div class="toast {t.kind}">{t.text}</div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    bottom: 36px;
    right: 16px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    z-index: 1000;
    max-width: 480px;
  }
  .toast {
    padding: 8px 12px;
    border-radius: 8px;
    font-size: 12px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.18);
    background: var(--bg-pane);
    color: var(--text-1);
    border: 1px solid var(--border-subtle);
    animation: slide-in 0.15s ease-out;
    word-break: break-all;
  }
  .toast.error { border-color: var(--danger); color: var(--danger); }
  .toast.ok { border-color: var(--ok); }
  @keyframes slide-in {
    from { transform: translateY(6px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
</style>
