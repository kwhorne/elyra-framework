#!/usr/bin/env bash
# The example app as an MCP server (RFC 0003), checked with the official MCP
# Inspector's CLI — a client we didn't write, speaking the `initialize` era.
#
#   scripts/mcp-inspector.sh target/debug/elyra-example
#
# Needs node (for npx) and jq. The app runs headless: nothing else is open.
set -euo pipefail

app="${1:?usage: scripts/mcp-inspector.sh <path to the example binary>}"
inspector="@modelcontextprotocol/inspector@2.9.0"

# The Inspector takes `--mcp` for one of its own flags, so the app is told by
# its environment instead — the same switch.
mcp() {
  npx -y "$inspector" --cli "$app" -e ELYRA_MCP=stdio "$@"
}

fail() {
  echo "mcp-inspector: $*" >&2
  exit 1
}

echo "== tools/list"
tools=$(mcp --method tools/list | jq -c '[.tools[].name]')
[ "$tools" = '["add_todo","delete_todo","filter_todos","list_todos"]' ] || fail "tools: $tools"

echo "== tools/call add_todo"
title="from the inspector $$"
added=$(mcp --method tools/call --tool-name add_todo --tool-arg "title=$title")
[ "$(jq -r .isError <<<"$added")" = false ] || fail "add_todo: $added"
[ "$(jq -r .structuredContent.title <<<"$added")" = "$title" ] || fail "add_todo: $added"

echo "== resources/list"
uris=$(mcp --method resources/list | jq -c '[.resources[].uri]')
[ "$uris" = '["app://filter_todos","app://list_todos"]' ] || fail "resources: $uris"

echo "== resources/read app://list_todos"
todos=$(mcp --method resources/read --uri app://list_todos | jq -r '.contents[0].text')
jq -e --arg t "$title" 'any(.[]; .title == $t)' <<<"$todos" >/dev/null ||
  fail "the new todo isn't in the resource: $todos"

# A live command with an argument is a resource template (RFC 0004), read with
# the argument filled in — and a value that doesn't fit is refused.
echo "== resources/templates/list"
templates=$(mcp --method resources/templates/list | jq -c '[.resourceTemplates[].uriTemplate]')
[ "$templates" = '["app://filter_todos{?done}"]' ] || fail "templates: $templates"

echo "== resources/read app://filter_todos?done=false"
open_todos=$(mcp --method resources/read --uri 'app://filter_todos?done=false' | jq -r '.contents[0].text')
jq -e --arg t "$title" 'any(.[]; .title == $t) and all(.[]; .done == false)' <<<"$open_todos" >/dev/null ||
  fail "the open todos: $open_todos"
refused=$(mcp --method resources/read --uri 'app://filter_todos?done=maybe' 2>&1 || true)
grep -q "Invalid arguments" <<<"$refused" || fail "done=maybe was read: $refused"

# The Inspector's CLI can't ask its user anything, so a tool that needs
# confirmation must refuse rather than run.
echo "== tools/call delete_todo (needs confirmation)"
id=$(jq -r .structuredContent.id <<<"$added")
# (The Inspector exits non-zero on a tool error; the result is still on stdout.)
refused=$(mcp --method tools/call --tool-name delete_todo --tool-arg "id=$id" || true)
[ "$(jq -r .isError <<<"$refused")" = true ] || fail "delete_todo ran: $refused"
jq -r '.content[0].text' <<<"$refused" | grep -q "can't ask" || fail "delete_todo: $refused"
todos=$(mcp --method resources/read --uri app://list_todos | jq -r '.contents[0].text')
jq -e --argjson id "$id" 'any(.[]; .id == $id)' <<<"$todos" >/dev/null ||
  fail "the todo is gone: $todos"

echo "mcp-inspector: ok"
