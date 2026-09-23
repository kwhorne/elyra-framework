#!/usr/bin/env bash
# Smoke test for `rata make:resource` (RFC 0001): takes a freshly scaffolded
# project (`rata new <name> --elyra <checkout>/framework`) through the whole
# resource workflow, and fails on the first thing a user would trip over.
#
#   scripts/smoke-resource.sh <project-dir> <rata-binary>
#
# 1. Without the prerequisites it refuses, and writes nothing.
# 2. `--generate` a Team, then a Customer with every field type that references it.
# 3. Wire main.rs the way rata's hint says; after that, no hint is left.
# 4. The generated Rust: `cargo test`, `clippy -D warnings`, `cargo fmt --check`.
# 5. Migrate, seed and roll back a real SQLite database.
# 6. `rata codegen`, then the frontend build.
# 7. The views in headless Chrome against a fake backend (scripts/smoke/harness.js).
#    Skipped when no Chrome is found, unless SMOKE_REQUIRE_BROWSER=1 (CI).
set -euo pipefail

app="$(cd "$1" && pwd)"
rata="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
here="$(cd "$(dirname "$0")" && pwd)"
name="$(basename "$app")"
cd "$app"
# The app's binary, wherever cargo builds it.
bin="${CARGO_TARGET_DIR:-$app/target}/debug/$name"

step() { printf '\n==> %s\n' "$*"; }
fail() { printf 'smoke: %s\n' "$*" >&2; exit 1; }

TEAM=(name:string:unique)
CUSTOMER=(name:string email:email:unique 'phone:string?' 'bio:text?' active:bool=true
  credit:float=0 visits:integer:index 'born:date?' 'meta:json?' team_id:references:Team)

step "prerequisites are checked before anything is written"
if "$rata" make:resource Team --generate "${TEAM[@]}" >out.txt 2>&1; then
  fail "make:resource succeeded without the database feature"
fi
grep -q 'features = \["database"\]' out.txt || fail "no database hint: $(cat out.txt)"
grep -q 'serde_json' out.txt || fail "no serde_json hint: $(cat out.txt)"
[ ! -e src/resources/team ] || fail "files were written despite the refusal"

step "add them"
python3 - <<'PY'
import pathlib, re
p = pathlib.Path("Cargo.toml")
s = p.read_text()
s = re.sub(r'^elyra = \{ (.*) \}$', r'elyra = { \1, features = ["database"] }', s, count=1, flags=re.M)
s += 'serde_json = "1"\n\n[dev-dependencies]\ntokio = { version = "1", features = ["macros", "rt-multi-thread"] }\n'
p.write_text(s)
PY

step "generate Team and Customer"
"$rata" make:resource Team --generate "${TEAM[@]}"
"$rata" make:resource Customer --generate "${CUSTOMER[@]}" | tee out.txt
grep -q 'mod resources;' out.txt || fail "no wiring hint for main.rs"
if "$rata" make:resource Customer --generate name:string >out.txt 2>&1; then
  fail "a second --generate overwrote the model without --force"
fi

step "wire main.rs as the hint says"
python3 - <<'PY'
import pathlib
p = pathlib.Path("src/main.rs")
s = p.read_text()
s = s.replace("use elyra::", "mod resources;\n\nuse elyra::", 1)
s = s.replace(
    "        .commands(commands![",
    "        .commands(resources::commands())\n"
    "        .migrations(resources::migrations())\n"
    "        .seeders(resources::seeders())\n"
    "        .allow_abilities(resources::abilities())\n"
    "        .commands(commands![",
    1,
)
p.write_text(s)
PY
"$rata" resources:sync | tee out.txt
if grep -q 'once' out.txt; then fail "wiring still incomplete: $(cat out.txt)"; fi

step "the generated Rust"
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check

step "migrate, seed and roll back SQLite"
db="$(mktemp -d)/smoke.db"
export DATABASE_URL="sqlite://$db?mode=rwc"
cargo build
ELYRA_MIGRATE=up "$bin"
ELYRA_SEED=1 "$bin"
python3 - "$db" <<'PY'
import sqlite3, sys
db = sqlite3.connect(sys.argv[1])
count = lambda t: db.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0]
teams, customers = count("teams"), count("customers")
assert (teams, customers) == (20, 20), f"teams={teams} customers={customers}"
orphans = db.execute(
    "SELECT COUNT(*) FROM customers WHERE team_id NOT IN (SELECT id FROM teams)"
).fetchone()[0]
assert orphans == 0, f"{orphans} customers point at no team"
print(f"seeded {teams} teams and {customers} customers")
PY
ELYRA_MIGRATE=down "$bin"
python3 - "$db" <<'PY'
import sqlite3, sys
tables = {r[0] for r in sqlite3.connect(sys.argv[1]).execute("SELECT name FROM sqlite_master WHERE type='table'")}
assert not tables & {"teams", "customers"}, f"left behind: {tables}"
print("rolled back")
PY
unset DATABASE_URL

step "codegen and the frontend build"
"$rata" codegen
grep -q 'customers_index(query: CustomerQuery): Promise<Page<Customer>>' app/src/bindings.ts ||
  fail "bindings lack the typed customers_index"
grep -q 'export type JsonValue' app/src/bindings.ts || fail "bindings lack JsonValue"
(cd app && npm install --no-audit --no-fund && npm run build 2>&1 | tee ../build.txt)
if grep -qi 'warn' build.txt; then fail "the frontend build warns: $(grep -i warn build.txt)"; fi

step "the views in a browser"
chrome="${CHROME:-}"
for candidate in google-chrome google-chrome-stable chromium chromium-browser \
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"; do
  [ -n "$chrome" ] && break
  if command -v "$candidate" >/dev/null 2>&1 || [ -x "$candidate" ]; then chrome="$candidate"; fi
done
if [ -z "$chrome" ]; then
  [ "${SMOKE_REQUIRE_BROWSER:-0}" = 1 ] && fail "no Chrome found (set CHROME)"
  echo "no Chrome found — skipping the browser check"
  exit 0
fi
cp "$here/smoke/harness.js" app/src/harness.js
cp app/src/main.js app/src/main.js.orig
{ echo 'import "./harness.js";'; cat app/src/main.js.orig; } >app/src/main.js
(cd app && npx vite build --outDir dist-harness --emptyOutDir >/dev/null)
mv app/src/main.js.orig app/src/main.js
rm app/src/harness.js

port=$((20000 + RANDOM % 20000))
(cd app/dist-harness && exec python3 -m http.server "$port" >/dev/null 2>&1) &
server=$!
trap 'kill $server 2>/dev/null || true' EXIT
sleep 1
profile="$(mktemp -d)"
# Chrome can linger after dumping the DOM (the fake event long-poll is still
# open), so it gets a hard deadline; the DOM it wrote is what's checked.
perl -e 'alarm shift; exec @ARGV' 150 "$chrome" --headless=new --no-sandbox --disable-gpu --disable-dev-shm-usage \
  --use-mock-keychain --password-store=basic --user-data-dir="$profile" \
  --virtual-time-budget=60000 --dump-dom "http://127.0.0.1:$port/" >dom.html 2>/dev/null || true
python3 - <<'PY'
import html, re, sys
dom = open("dom.html", encoding="utf-8").read()
m = re.search(r'<pre id="harness">(.*?)</pre>', dom, re.S)
if not m:
    sys.exit("smoke: the browser scenario never finished:\n" + dom[:2000])
lines = html.unescape(m.group(1)).splitlines()
print("\n".join(lines))
failed = [l for l in lines if l.startswith("FAIL")]
if failed or not lines:
    sys.exit(f"smoke: {len(failed)} browser check(s) failed")
print(f"{len(lines)} browser checks passed")
PY
