<script>
  import { TreeView } from "@elyra/ui";

  // In an app: `(node) => api.folders_children(node.id)`, an array or a Page<T>.
  const folders = {
    clients: [{ id: "acme", label: "Acme AS", hasChildren: true }, { id: "kunde", label: "Kunde AS", hasChildren: true }],
    acme: [{ id: "acme-2026", label: "2026" }, { id: "acme-2025", label: "2025" }],
    kunde: [],
  };
  const load = (node) => new Promise((resolve) => setTimeout(() => resolve(folders[node.id] ?? []), 600));

  const roots = [
    { id: "clients", label: "Clients", hasChildren: true },
    { id: "templates", label: "Templates", children: [{ id: "invoice", label: "Invoice" }, { id: "reminder", label: "Reminder" }] },
  ];
</script>

<div class="box">
  <TreeView nodes={roots} {load} persist="docs-folders" label="Folders" />
</div>

<style>
  .box { width: min(100%, 300px); }
</style>
