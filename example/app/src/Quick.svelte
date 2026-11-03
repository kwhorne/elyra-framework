<script>
  // The tray's popover (RFC 0019): the todos at a glance, and one to add.
  // A click elsewhere or Esc hides it; it stays loaded, so the list is live.
  import { appWindow } from "@elyra/runtime";
  import { Button, Input } from "@elyra/ui";
  import { api, live } from "./bindings";

  const todos = live.list_todos();
  let title = $state("");

  // Shown again: ready to type.
  appWindow.onState((s) => {
    if (s.label === "quick" && s.visible && s.focused) document.querySelector("input")?.focus();
  });

  async function add(event) {
    event.preventDefault();
    if (!title.trim()) return;
    await api.add_todo(title.trim());
    title = "";
  }
</script>

<main>
  <h1>Todos</h1>
  <ul>
    {#each ($todos.value ?? []).slice(-6) as todo (todo.id)}
      <li>{todo.title}</li>
    {:else}
      <li class="none">Nothing yet.</li>
    {/each}
  </ul>
  <form onsubmit={add}>
    <Input bind:value={title} placeholder="New todo" aria-label="New todo" />
    <Button type="submit">Add</Button>
  </form>
</main>

<style>
  main { display: grid; gap: 12px; padding: 14px; font: 14px system-ui, sans-serif; }
  h1 { font-size: 15px; margin: 0; }
  ul { margin: 0; padding-left: 18px; display: grid; gap: 4px; }
  .none { color: var(--elyra-muted, #888); list-style: none; margin-left: -18px; }
  form { display: flex; gap: 8px; align-items: end; }
</style>
