# Coding agents

What a coding agent (Claude Code, Codex, Cursor, Gemini CLI, Elyra
Workspace…) needs to work well on an Elyra app, the way
[Laravel Boost](https://laravel.com/docs/boost) does it for Laravel:
guidelines it reads up front, skills it loads when a task needs them, and an
MCP server that answers questions about the app.

This is about agents **writing** the app. An agent **using** the app, calling
its commands as tools, is [the app as an MCP server](mcp.md).

## In two lines

```bash
rata ai:install     # AGENTS.md block, skills, and `rata mcp dev` in .mcp.json
rata ai:update      # refresh them after upgrading rata
```

## What `ai:install` writes

- **Guidelines** in `AGENTS.md`, between `<elyra-framework-guidelines>` and
  `</elyra-framework-guidelines>`: commands and abilities, middleware, the
  container, models and migrations, live queries, `rata codegen` after a type
  changes, which `rata` generator to use, and the checks. Only the block is
  rata's. What you write around it stays, and `ai:update` replaces only the
  block. A `CLAUDE.md` gets the same block, unless it imports `AGENTS.md`
  (`@AGENTS.md`).
- **Skills** in `.agents/skills/` and `.claude/skills/`, in the
  [Agent Skills](https://agentskills.io) format, loaded only when they apply:

  | Skill | For |
  | --- | --- |
  | `elyra-commands` | commands, `Ctx`, validation, abilities, middleware, events, `TestApp` |
  | `elyra-models-and-migrations` | `#[derive(Model)]`, the query builder, relations, casts, factories, SQL and Rust migrations |
  | `elyra-resources` | `rata make:resource`, the registries, changing a generated resource |
  | `elyra-live-queries` | `#[command(live)]`, `live.*`, `depends_on` / `invalidate`, testing |
  | `elyra-laravel-backend` | `Backend`, `Auth`, Laravel's answers, broadcasting, the outbox, notifications |

  A skill file ends with rata's marker. One without it is yours: rata says
  so and leaves it, so delete it to get the framework's back.
- **The `elyra-framework` server** in `.mcp.json`: `rata mcp dev`, next to the
  servers already there (Laravel Boost's, say). Claude Code reads the file,
  and Elyra Workspace gives its servers to every agent once you allow it.

`ai:update` refreshes only what's there: a removed skill or server stays
removed. Run it after upgrading rata, so the guidelines match the framework.

## `rata mcp dev`

An MCP server over stdio for the agent working on the code. Every tool only
reads: nothing is migrated, run or written, and a SQLite database is opened
read-only.

| Tool | What it answers |
| --- | --- |
| `app_info` | the crate and its version, the framework's version and features, the frontend and bindings paths, the database driver (never the URL), every `#[command]` with `live`, `can` and `middleware`, every `#[derive(Model)]` with its table, the resources |
| `database_schema` | the tables, and each column's type, null, default and key (`table` for one) |
| `migration_status` | the SQL and Rust migrations, applied (with their batch) or pending, against `_elyra_migrations`, without creating it |
| `last_error` | the newest `ERROR` in the app's log, with the entries before it |
| `read_log_entries` | the last entries (`count`, at most 200), from a `level` up |
| `list_commands` | rata's own commands and flags |
| `search_docs` | these guides, for this version, embedded in rata: the best sections with their file and heading |

- **The code** is read with `syn`, as `make:resource` reads a model, so
  `app_info` needs no build.
- **The log** is the one `LogProvider` writes (`to_app_dir("My App")` →
  `<config dir>/my-app/app.log`, or `to_path(..)`), found from the source, then
  from the bundle name and the crate. Rotated `app.log.1` is read too. Give
  `path` when it's elsewhere.
- **The database** comes from `[database].url` or `DATABASE_URL`, as for
  `rata migrate`. A SQLite file that isn't there isn't created.
- **Protocol:** clients that open with `initialize` (2025-11-25, 2025-06-18,
  2025-03-26), and 2026-07-28 requests carrying their version in `_meta`.

## The Laravel half

When the app is the desktop half of a Laravel app
([a Laravel backend](backend.md)), the Composer package
[`elyracode/elyra-backend`](../laravel/README.md) in this repository gives the
Laravel app's agents the same knowledge from the other side. Laravel Boost
picks up its guidelines and its `elyra-desktop-backend` skill: what the
desktop expects of the API (the Sanctum token routes, `422`s per field, the
status codes, `Idempotency-Key`, `If-Unmodified-Since`, broadcasting,
notifications), and that changing an API resource means changing the
desktop's types and running `rata codegen`. The package also has the routes
and the idempotency middleware.

A project with both halves then gives an agent both servers: Boost's
`laravel-boost` for the API, and `elyra-framework` for the desktop.

## Related

- [AI agents (MCP)](mcp.md) — the app's own commands as tools
- [Ratatosk CLI](cli.md) · [A Laravel backend](backend.md)
