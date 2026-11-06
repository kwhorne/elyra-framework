#!/usr/bin/env python3
"""The running app for a coding agent (RFC 0026), end to end: `rata mcp dev`
against the example app's debug build, as an agent would use it. CI runs it
on Linux under Xvfb, so the picture there is taken for real; it runs on a
Mac too.

    scripts/running-app.py <rata> <the example's folder>

The example app must be running (a debug build). Exits non-zero, saying
what failed, when an answer isn't what it should be.
"""
import base64
import json
import re
import struct
import subprocess
import sys
import time

rata, example = sys.argv[1], sys.argv[2]
server = subprocess.Popen(
    [rata, "mcp", "dev"], cwd=example, stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True
)
calls = 0


def rpc(method, params=None):
    global calls
    calls += 1
    server.stdin.write(json.dumps({"jsonrpc": "2.0", "id": calls, "method": method, "params": params or {}}) + "\n")
    server.stdin.flush()
    return json.loads(server.stdout.readline())


def call(name, **args):
    result = rpc("tools/call", {"name": name, "arguments": args})["result"]
    text = "\n".join(c.get("text", "") for c in result["content"] if c["type"] == "text")
    return result, text


def check(ok, what, detail=""):
    print(("ok   " if ok else "FAIL ") + what)
    if not ok:
        print(detail)
        sys.exit(1)


rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "ci", "version": "1"}})

# The app takes a moment to open its window and its dev endpoint.
deadline = time.time() + 60
while True:
    result, windows = call("app_windows")
    if not result.get("isError") and "main" in windows:
        break
    if time.time() > deadline:
        check(False, "the app's windows", windows)
    time.sleep(1)
check(re.search(r"^main .* shown", windows, re.M) is not None, "app_windows lists the shown main window", windows)

# The page's script runs once the page has loaded: wait for a snapshot.
deadline = time.time() + 30
while True:
    result, snapshot = call("window_snapshot", window="main")
    if not result.get("isError"):
        break
    if time.time() > deadline:
        check(False, "a snapshot", snapshot)
    time.sleep(1)
check(snapshot.startswith("route: "), "the snapshot starts with the route", snapshot[:300])
check('heading "greet(name)"' in snapshot, "the snapshot has the page's headings", snapshot[:1500])
# What the window shows in the app's own words: its live todo list (RFC 0013).
check(
    "showing (the window's live queries, newest first):\n- list_todos []" in snapshot,
    "under the snapshot, the window's live queries",
    snapshot[-600:],
)

name = re.search(r'textbox "name"[^\n]*\[ref=(e\d+)\]', snapshot)
check(name is not None, "the name field has a ref", snapshot[:1500])
result, after = call("window_act", window="main", action="type", ref=name.group(1), text="Ada")
check(re.search(r'textbox "name" value="Ada"', after) is not None, "typing sets the field", after[:1500])

invoke = re.search(r'heading "greet\(name\)"[\s\S]*?button "invoke" \[ref=(e\d+)\]', after)
check(invoke is not None, "greet's button has a ref", after[:1500])
result, after = call("window_act", window="main", action="click", ref=invoke.group(1))
check('text "Hello, Ada!"' in after, "clicking calls greet, and the page after shows its answer", after[:2000])

result, calls_text = call("app_calls", window="main")
check(re.search(r"^greet  ok", calls_text, re.M) is not None, "app_calls has the greet call", calls_text)

result, stale = call("window_act", window="main", action="click", ref="e9999")
check(result.get("isError") and "take a snapshot first" in stale, "a ref it didn't give is an error", stale)

result, size = call("window_screenshot", window="main")
check(not result.get("isError"), "a picture of the main window", size)
image = next(c for c in result["content"] if c["type"] == "image")
png = base64.b64decode(image["data"])
check(png[:8] == b"\x89PNG\r\n\x1a\n", "the picture is a PNG")
width, height = struct.unpack(">II", png[16:24])
said = re.search(r"(\d+)×(\d+)", size)
check(
    said is not None and (width, height) == (int(said.group(1)), int(said.group(2))) and 0 < width <= 1280,
    f"the picture is {width}×{height}, as said, and at most 1280 wide",
    size,
)
# Not all one colour: the page drew something.
check(len(png) > 2000, f"the picture has the page in it ({len(png)} bytes)")

server.stdin.close()
server.wait()
print("the running app answers as an agent needs")
