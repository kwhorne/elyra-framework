<script>
  import { Field, Input } from "@elyra/ui";
</script>

<Field label="Email"><Input type="email" placeholder="ada@example.com" /></Field>
<Field label="Credit limit"><Input type="number" min="0" step="100" value="5000" /></Field>
<Field label="Find"><Input type="search" placeholder="Search customers…" /></Field>
<Field label="Disabled"><Input value="Can't change this" disabled /></Field>
