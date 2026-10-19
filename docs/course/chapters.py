"""Freddy Learning Elyra Framework: the chapters.

Each chapter is a dict; build.py wraps them in the same page shell. Only <main>
is rendered on elyracode.com, so anything a reader must see belongs in a chapter,
not in the course intro. Freddy and his notes app are the story; everything the
course says the framework does is in the guide for the version it names.
"""

COURSE_TITLE = "Freddy Learning Elyra Framework"

COURSE_LEAD = (
    "Freddy has a notes app on the web, built with Laravel and Svelte. In fifteen "
    "chapters he builds its desktop half with Elyra Framework 0.16: a compiled app "
    "with its own database, then the same app signed in to his Laravel backend, "
    "following the web, working offline and showing Laravel's notifications."
)

COURSE_INTRO = """                <p>
                    The course follows one app from <code>rata new</code> to a release. Each
                    chapter names the guide pages it is built on, so you can always read the
                    whole story there.
                </p>"""

CHAPTERS = []

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 1,
    "title": "Why a desktop app at all",
    "summary": "What Elyra Framework is, the Laravel map, and one honest note before we start.",
    "lead": (
        "Freddy's notes app works fine in a browser. So why would he want a desktop "
        "app, and why this framework? This chapter answers both, and maps what he "
        "already knows from Laravel onto what he is about to learn."
    ),
    "body": r"""                <h2 id="freddy">Freddy and his notes</h2>
                <p>
                    If you followed <em>Freddy the Notetaker</em>, you know Freddy: he built a
                    notes app with Laravel, Inertia, Svelte and SQLite. It runs in a browser,
                    and it is good. But Freddy keeps wanting three things a browser tab is bad
                    at: a window that is there when he presses a shortcut, notes he can read on
                    a train with no network, and a nudge when a colleague mentions him, even
                    with the tab closed.
                </p>
                <p>
                    That is a desktop app. In this course Freddy builds one with Elyra
                    Framework, first on its own, with its own database on his machine, and then
                    as the desktop half of the Laravel app he already has.
                </p>
                <div class="callout">
                    <strong>One honest note before we start.</strong> Freddy, his notes app and
                    his colleagues are a story to hang the chapters on. What the course says the
                    framework does comes from the
                    <a href="../overview.html">Elyra Framework guide</a> for version 0.16.0, and
                    every chapter names the pages it is built on. Where the guide does not say
                    something, the course says so instead of guessing; the first time is in the
                    next chapter, where the guide does not say how to install <code>rata</code>.
                    Elyra Framework is young, and it is not published to crates.io or npm: an
                    app depends on a tagged release in the repository.
                </div>

                <h2 id="what-it-is">What Elyra Framework is</h2>
                <p>
                    In its own words, Elyra Framework is <em>a Rust + Svelte 5 framework for
                    hyper-responsive desktop apps</em>, with <em>Laravel's ergonomics &mdash;
                    container, providers, middleware, a typed bridge &mdash; but compiled, binary,
                    and without runtime overhead</em>. The backend is Rust, compiled to a single
                    native binary with no runtime interpreter. The window is a webview, and in it
                    runs a Svelte 5 app. Between the two is Elyra's own bridge: MessagePack over an
                    <code>elyra://localhost</code> custom protocol, with no JSON in the hot path.
                </p>
                <p>
                    The command-line tool is <strong>Ratatosk</strong>, <code>rata</code>: the
                    squirrel that runs messages up and down Yggdrasil, between the Rust root and the
                    Svelte crown. It scaffolds projects, generates code, runs migrations and
                    installs what coding agents need. You will meet it in every chapter.
                </p>

                <h2 id="the-laravel-map">The Laravel map</h2>
                <p>
                    Freddy's best asset is that he already knows Laravel. The guide's
                    <a href="../architecture.html">architecture page</a> maps it, and this course
                    follows the same map:
                </p>
                <table>
                    <thead>
                        <tr><th>Laravel</th><th>Elyra</th><th>Chapter</th></tr>
                    </thead>
                    <tbody>
                        <tr><td>Application + container</td><td><code>App</code> + <code>Container</code> (<code>ctx.get::&lt;T&gt;()</code>)</td><td>4</td></tr>
                        <tr><td>ServiceProvider</td><td><code>Provider</code> (<code>register</code> / <code>boot</code>)</td><td>4</td></tr>
                        <tr><td>routes/web.php</td><td><code>commands![...]</code></td><td>3</td></tr>
                        <tr><td>Controller action</td><td><code>#[command] async fn</code></td><td>3</td></tr>
                        <tr><td>Middleware</td><td>a pipeline in <code>CommandRegistry::dispatch</code></td><td>4</td></tr>
                        <tr><td>Broadcasting + Echo</td><td><code>EventBus</code> + the <code>channel()</code> store</td><td>4, 12</td></tr>
                        <tr><td>Facades</td><td>the generated <code>api.*</code></td><td>3</td></tr>
                        <tr><td>HTTP client (<code>Http::</code>)</td><td><code>Http</code></td><td>11</td></tr>
                        <tr><td>Sanctum + the API, from another app</td><td><code>Backend</code>, <code>Auth</code>, working offline with an outbox</td><td>11&ndash;14</td></tr>
                        <tr><td>Eloquent</td><td><code>#[derive(Model)]</code></td><td>5</td></tr>
                        <tr><td>Artisan</td><td>Ratatosk (<code>rata</code>)</td><td>all of them</td></tr>
                        <tr><td>Blade / Livewire</td><td>Svelte 5 (runes)</td><td>3, 6, 7</td></tr>
                    </tbody>
                </table>
                <p>
                    Freddy reads the last row twice. There is no Blade here: the page is a Svelte
                    app that calls Rust, the way an Inertia page calls a controller, except that the
                    call never leaves the machine.
                </p>

                <h2 id="the-page-is-untrusted">The page is untrusted</h2>
                <p>
                    One idea from the architecture page is worth carrying through the whole
                    course: <em>the webview is untrusted</em>. Anything running in the page, your
                    code, a dependency, injected content, can reach the bridge. So Elyra gates it
                    before dispatch with three mechanisms: a random per-run token injected into the
                    webview before any page script runs, origin isolation with no CORS headers at
                    all in a production build, and a capability model where destructive routes are
                    opt-in. Request bodies are capped in size and nesting depth too.
                </p>
                <p>
                    You will see this again and again: a command that deletes can be marked so the
                    page cannot call it until the app grants it (chapter 3), the sign-in token lives
                    in the keychain and never reaches the page (chapter 11), and a notification's
                    full URL is never opened by itself (chapter 14).
                </p>

                <h2 id="the-plan">Freddy's plan</h2>
                <ol>
                    <li>Chapters 2&ndash;4: a project, commands the page can call, and the container.</li>
                    <li>Chapters 5&ndash;7: a database on the desktop, screens generated from a model, and lists that update themselves.</li>
                    <li>Chapters 8&ndash;10: windows, menus, the tray, the settings store and secrets, AI in the app, and coding agents that know the framework.</li>
                    <li>Chapters 11&ndash;14: the same app as the desktop half of his Laravel app, signed in, following the web, working offline and showing notifications.</li>
                    <li>Chapter 15: tests, a release, and the updater.</li>
                </ol>""",
    "learned": [
        "Elyra Framework is Rust and Svelte 5, compiled to one native binary, with Laravel's building blocks.",
        "The bridge is MessagePack over an <code>elyra://localhost</code> protocol, and the page on the other side of it is treated as untrusted.",
        "<code>rata</code> is the framework's Artisan.",
        "The framework is not on crates.io or npm; an app depends on a tagged release.",
    ],
    "next": "Freddy installs <code>rata</code> and opens his first window.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 2,
    "title": "rata new and the first window",
    "summary": "Getting rata, scaffolding a project, and what each file is for.",
    "lead": (
        "Every Elyra app starts the same way: <code>rata new</code>, a frontend build, "
        "<code>rata codegen</code>, and <code>cargo run</code>. Freddy does all four and "
        "then looks at what he got."
    ),
    "body": r"""                <h2 id="getting-rata">Getting rata</h2>
                <p>
                    The guide starts after this step: it does not say how to install
                    <code>rata</code>. What the repository does say is that <code>rata</code> is the
                    binary of the <code>ratatosk</code> crate in the framework's repository, and
                    since nothing is published to a registry, Cargo can install it straight from a
                    release tag. This is plain Cargo, not a command from the guide, and it was run
                    for this course against v0.16.0:
                </p>
                <pre><code>cargo install --locked --git https://github.com/kwhorne/elyra-framework --tag v0.16.0 ratatosk</code></pre>
                <p>
                    You need a Rust toolchain for that, and Node with npm for the frontend.
                    <code>rata help</code> lists the commands.
                </p>

                <h2 id="rata-new">rata new</h2>
                <pre><code>rata new freddy-notes
cd freddy-notes</code></pre>
                <p>
                    That generates a self-contained workspace. Freddy's has these files, which is
                    what <code>rata new</code> 0.16.0 wrote when this course was made:
                </p>
                <pre><code>Cargo.toml          # depends on the framework's release tag
elyra.toml          # rata's configuration
src/main.rs         # the app: App::new(), commands, providers
app/                # the Svelte 5 frontend (Vite)
  index.html
  package.json      # @elyra/runtime from the release
  src/main.js
  src/App.svelte
  src/Router.svelte
  src/routes.js
  src/pages/Home.svelte
  src/pages/NotFound.svelte
  src/resources/index.js
  src/theme.js
  src/app.css</code></pre>
                <p>
                    Two lines in it matter more than they look. <code>Cargo.toml</code> depends on
                    the framework by tag, and <code>app/package.json</code> points
                    <code>@elyra/runtime</code> at the tarball attached to the same release, because
                    npm cannot install a subdirectory of a git repository:
                </p>
                <pre><code>elyra = { git = "https://github.com/kwhorne/elyra-framework", tag = "v0.16.0" }

"@elyra/runtime": "https://github.com/kwhorne/elyra-framework/releases/download/v0.16.0/elyra-runtime-0.16.0.tgz"</code></pre>
                <p>
                    Upgrading the framework later means moving both to the new tag.
                    <code>rata new --elyra &lt;path&gt;</code> instead wires a project to a local
                    checkout of the framework, for working on the framework itself.
                </p>

                <h2 id="first-run">The first run</h2>
                <p>
                    <code>rata new</code> prints the next steps, and they are the guide's quick
                    start:
                </p>
                <pre><code>(cd app &amp;&amp; npm install &amp;&amp; npm run build)   # build the frontend
rata codegen                                # generate typed bindings
cargo run                                   # launch</code></pre>
                <p>
                    For this course the first three steps were run on a fresh project: the frontend
                    built, <code>rata codegen</code> wrote <code>app/src/bindings.ts</code>, and
                    the app compiled. <code>cargo run</code> then opens the window: titled
                    <code>freddy-notes</code>, as <code>src/main.rs</code> sets it, with a Svelte page
                    inside it and a single binary behind it.
                </p>
                <p>
                    While he works, Freddy does not want to rebuild the frontend for every change.
                    <code>rata dev</code> runs the frontend's dev server next to the app, so a change
                    to a Svelte file shows at once; the <a href="../cli.html">CLI page</a> has the
                    details. The <a href="../getting-started.html">getting started guide</a> walks
                    through the same steps.
                </p>

                <h2 id="what-runs-where">What runs where</h2>
                <p>
                    The architecture page explains what Freddy just started. The event loop and
                    the webview live on the main thread, which macOS requires. Commands run on an
                    async runtime, each on its own task, so a slow one never freezes the window. The
                    page talks to Rust only through the bridge, and Rust pushes to the page through
                    an event bus whose batches the frontend collects over a long-poll it holds
                    open.
                </p>
                <div class="callout">
                    <strong>Why it feels fast.</strong> There is no interpreter between Freddy's
                    code and the machine, and nothing on the bridge is JSON: arguments go over as a
                    compact MessagePack array and results come back as MessagePack maps. Chapter 3
                    is about writing the code on both ends of that bridge.
                </div>""",
    "learned": [
        "<code>rata</code> is the <code>ratatosk</code> crate's binary; the guide does not document installing it, and plain Cargo can install it from a release tag.",
        "<code>rata new</code> scaffolds a Rust app and a Svelte 5 frontend that depend on one release, by tag and by tarball.",
        "The first run is: build the frontend, <code>rata codegen</code>, <code>cargo run</code>; <code>rata dev</code> keeps the frontend live while you work.",
        "The window lives on the main thread, and each command on its own task.",
    ],
    "next": "Freddy writes his first command and calls it from Svelte.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 3,
    "title": "Commands: Rust that the page can call",
    "summary": "#[command], the Ctx, Result and kinds of failure, rata codegen, cancellation and gating.",
    "lead": (
        "A command is the compiled equivalent of a Laravel controller action. Freddy "
        "writes one, registers it, generates its TypeScript, and calls it from a Svelte "
        "page as if it were a local function."
    ),
    "body": r"""                <h2 id="a-command">A command</h2>
                <p>
                    Annotate an <code>async fn</code> with <code>#[command]</code> and register it
                    with <code>commands![...]</code>. This is the guide's own first example:
                </p>
                <pre><code>use elyra::{command, commands, App, Ctx};

#[command]
async fn greet(ctx: Ctx, name: String) -&gt; String {
    format!("Hello, {name}!")
}

App::new().commands(commands![greet, add]).run();</code></pre>
                <p>
                    <code>commands![...]</code> is Freddy's <code>routes/web.php</code>. The macro
                    turns the function into a type of the same name, which is why the function name
                    is also what you pass to the list.
                </p>
                <p>
                    The first parameter is always the context, <code>Ctx</code>, passed through
                    untouched. It is how a command reaches the container, which chapter 4 is about;
                    name it <code>_ctx</code> when a command does not need it. Arguments must
                    implement <code>serde::Deserialize</code> and <code>specta::Type</code>, and the
                    return type <code>serde::Serialize</code> and <code>specta::Type</code>. Structs
                    go over as named maps, so they arrive as plain JavaScript objects and survive
                    fields being reordered between versions.
                </p>

                <h2 id="calling-it">Calling it from Svelte</h2>
                <p>
                    The low-level call is <code>invoke</code> from <code>@elyra/runtime</code>. The
                    one Freddy will actually use is the typed facade that <code>rata codegen</code>
                    writes:
                </p>
                <pre><code>import { api } from "./bindings";
const greeting = await api.greet("world");</code></pre>
                <p>
                    <code>rata codegen</code> writes <code>app/src/bindings.ts</code> from the
                    app's commands: a TypeScript type for every Rust type they use, and an
                    <code>api</code> object whose methods call <code>invoke</code>. It is the
                    framework's Facade, generated, so a renamed field in Rust is a type error in
                    Svelte instead of a surprise at runtime. Run it after changing a command's
                    signature or a type it uses.
                </p>
                <p>
                    One detail from the <a href="../codegen.html">codegen page</a> that matters for
                    Freddy's note ids: 64-bit integers render as <code>number</code>, because on the
                    MessagePack wire they already arrive as JavaScript numbers.
                </p>

                <h2 id="when-it-fails">When it fails</h2>
                <p>
                    A command that can fail returns <code>Result&lt;T, E&gt;</code> where
                    <code>E: Display</code>. <code>Ok</code> becomes the value; <code>Err</code>
                    makes the frontend's promise reject with a <code>CommandError</code>. When a
                    command mixes kinds of failure, <code>elyra::Result&lt;T&gt;</code> is the
                    convenient shape: <code>?</code> converts a validation error bag, which reaches
                    the frontend as a <code>ValidationError</code> with its fields intact, and a query
                    error, and <code>elyra::Error::command(msg)</code> covers the rest.
                </p>
                <p>
                    The frontend can tell failures apart, because every error carries a kind:
                </p>
                <table>
                    <thead><tr><th>Kind</th><th>Meaning</th><th>Frontend type</th></tr></thead>
                    <tbody>
                        <tr><td><code>command</code></td><td>the command's own <code>Err</code></td><td><code>CommandError</code></td></tr>
                        <tr><td><code>validation</code></td><td>a validation error bag</td><td><code>ValidationError</code></td></tr>
                        <tr><td><code>panic</code></td><td>the handler panicked</td><td><code>CommandError</code></td></tr>
                        <tr><td><code>cancelled</code></td><td>cancelled from the frontend</td><td><code>CommandError</code></td></tr>
                        <tr><td><code>forbidden</code></td><td>missing token or capability</td><td><code>ForbiddenError</code></td></tr>
                        <tr><td>your own</td><td><code>Error::with_kind("archived", "&hellip;")</code></td><td><code>CommandError</code> with that kind</td></tr>
                    </tbody>
                </table>
                <p>
                    A panic does not hang the caller: every command runs on its own task, so a panic
                    becomes an error response with the panic message, and is logged. Chapter 11 adds
                    the kinds a Laravel backend's answers become, such as <code>offline</code> and
                    <code>unauthenticated</code>.
                </p>

                <h2 id="long-commands">Long commands: cancel and progress</h2>
                <p>
                    Freddy's export of all his notes to Markdown takes a while. The page can cancel
                    it with <code>invokeCancellable</code>; the Rust task is aborted at its next
                    <code>.await</code>, so a cancellable command should await now and then:
                </p>
                <pre><code>import { invokeCancellable } from "@elyra/runtime";

const job = invokeCancellable("build_report", opts);
onDestroy(() =&gt; job.cancel());
const report = await job.result;</code></pre>
                <p>
                    There is no special progress channel. The command emits on the event bus, and
                    the page listens with <code>channel()</code>, which chapter 4 introduces.
                </p>

                <h2 id="gating">Keeping the page away from a command</h2>
                <p>
                    Remember the untrusted page. One grant opens all commands to it, so a command
                    that should not be in that blanket is marked with an ability:
                </p>
                <pre><code>#[command(can = "posts.delete")]
async fn delete_post(ctx: Ctx, id: i64) -&gt; elyra::Result&lt;()&gt; {
    // ...
}

App::new()
    .commands(commands![delete_post])
    .allow_ability("posts.delete");   // or "posts.*"</code></pre>
                <p>
                    Without the grant, a call from the page answers 403 and names what is missing.
                    Calls from Rust, from providers, queue jobs and tests, are not affected. Freddy
                    marks his "delete every note" command this way and grants it only once he is
                    sure.
                </p>
                <p>
                    The whole story is on the <a href="../commands.html">commands page</a>, with
                    the wire format on <a href="../wire-format.html">its own page</a> and the
                    frontend side on the <a href="../frontend-runtime.html">frontend runtime
                    page</a>.
                </p>""",
    "learned": [
        "A command is <code>#[command] async fn</code> with <code>Ctx</code> first, registered with <code>commands![...]</code>.",
        "<code>rata codegen</code> writes <code>bindings.ts</code>: types and a typed <code>api.*</code>.",
        "<code>elyra::Result</code> and error kinds let the page tell a validation error from a panic, a cancellation or a refusal.",
        "<code>invokeCancellable</code> stops a long command at its next await.",
        "<code>#[command(can = \"...\")]</code> keeps a command away from the page until the app grants the ability.",
    ],
    "next": "Freddy meets the container, providers, middleware and the event bus.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 4,
    "title": "The container, providers, middleware and events",
    "summary": "Bindings and Ctx, providers' two phases, middleware around every command, and two kinds of events.",
    "lead": (
        "Freddy's commands start to need things: a configuration, a way to export, a "
        "log of who deleted what. In Laravel he would reach for the container, a "
        "service provider, middleware and events. Elyra has all four, with the same names."
    ),
    "body": r"""                <h2 id="the-container">The container and Ctx</h2>
                <p>
                    <code>Container</code> is a type-keyed registry of shared singletons. Freddy
                    rarely touches it directly: he binds with <code>App::bind</code> or in a
                    provider, and resolves through <code>Ctx</code>:
                </p>
                <pre><code>App::new()
    .bind(Config { greeting: "hi".into() })
    .commands(commands![greet]);

#[command]
async fn greet(ctx: Ctx, name: String) -&gt; String {
    let cfg = ctx.get::&lt;Config&gt;();        // Arc&lt;Config&gt;; panics if unbound
    format!("{} {name}", cfg.greeting)
}</code></pre>
                <p>
                    <code>get</code> panics with a clear message when nothing is bound, because a
                    missing binding is a wiring bug and should fail loudly. <code>try_get</code>
                    returns an <code>Option</code>, and <code>has</code> asks without building
                    anything. Bindings must be <code>Send + Sync + 'static</code>, since commands run
                    on tokio, and a later bind of the same type replaces the earlier one.
                </p>

                <h2 id="interfaces">Binding an interface</h2>
                <p>
                    Freddy's export can write Markdown files today and might upload somewhere
                    tomorrow. Laravel's <code>bind(Interface::class, Impl::class)</code> is
                    <code>bind_as</code>, under a trait object:
                </p>
                <pre><code>trait Exporter: Send + Sync { fn export(&amp;self, notes: &amp;[Note]) -&gt; Result&lt;()&gt;; }

App::new().bind_as::&lt;dyn Exporter&gt;(Arc::new(MarkdownExporter::new()));

#[command]
async fn export_all(ctx: Ctx) -&gt; Result&lt;()&gt; {
    let notes = /* ... */;
    ctx.get::&lt;dyn Exporter&gt;().export(&amp;notes)
}</code></pre>
                <p>
                    <code>Exporter</code> and <code>MarkdownExporter</code> are Freddy's; the
                    pattern is the guide's, which uses a <code>Mailer</code>. Swapping the
                    implementation, a fake in a test for instance, happens in one place and never
                    touches a command.
                </p>

                <h2 id="providers">Providers, in two phases</h2>
                <p>
                    A <code>Provider</code> is Laravel's ServiceProvider, and it has the same two
                    phases. <code>register</code> binds services and runs for every provider first,
                    so it must not resolve anything; <code>boot</code> runs after everything is
                    registered, with a populated context:
                </p>
                <pre><code>struct ConfigProvider;

impl Provider for ConfigProvider {
    fn register(&amp;self, c: &amp;mut Container) {
        c.bind(Config { greeting: "hi".into() });
    }
    fn boot(&amp;self, ctx: &amp;Ctx) {
        let _ = ctx.get::&lt;Config&gt;();   // the container is complete here
    }
}

App::new().provider(ConfigProvider).run();</code></pre>
                <p>
                    When a service needs another one to be built, <code>bind_lazy</code> builds it
                    the first time it is resolved, after every provider has registered, so the
                    order of providers stops mattering. A cycle between lazy bindings panics with the
                    chain instead of deadlocking. The <a href="../container-and-providers.html">container
                    page</a> lists the full lifecycle, from the first bind to the window opening.
                </p>

                <h2 id="middleware">Middleware around every command</h2>
                <p>
                    Middleware wraps command dispatch in an onion, so logging, timing, auth and
                    rate limiting live in one place instead of in every command. A middleware calls
                    <code>next.run(ctx, req)</code> to continue, or returns its own result to stop
                    the call. Global middleware runs in registration order, outermost first:
                </p>
                <pre><code>App::new().middleware(Timing).commands(commands![..]).run();</code></pre>
                <p>
                    Freddy wants an audit line only when a note is deleted. That is per-command
                    middleware, Laravel's middleware aliases:
                </p>
                <pre><code>App::new()
    .middleware_alias("audit", AuditLog::new())
    .commands(commands![delete_note]);

#[command(middleware = ["audit"])]
async fn delete_note(ctx: Ctx, id: i64) -&gt; Result&lt;()&gt; { /* ... */ }</code></pre>
                <p>
                    Two details from the <a href="../middleware.html">middleware page</a> are worth
                    keeping. An unknown name stops the app at startup, because a misspelt name must
                    never mean a command runs without its auth check. And middleware is not the same
                    as an ability from chapter 3: an ability decides whether the page may call a
                    command at all, while middleware wraps every call, from the page or from Rust.
                    A request also says who is calling, the app's own webview or an AI agent over
                    MCP, which chapter 10 comes back to.
                </p>

                <h2 id="the-event-bus">The EventBus: Rust pushes to the page</h2>
                <p>
                    The <code>EventBus</code> is Elyra's Broadcasting. Rust owns the state, and the
                    page subscribes to changes instead of polling. Emitting is non-blocking and works
                    from any thread:
                </p>
                <pre><code>ctx.get::&lt;EventBus&gt;().emit("export:progress", &amp;42u32).ok();</code></pre>
                <p>
                    On the page, <code>channel(name)</code> returns a Svelte-readable store, so
                    Freddy's progress bar is just <code>$progress</code>:
                </p>
                <pre><code>&lt;script&gt;
  import { channel } from "@elyra/runtime";
  const progress = channel("export:progress");
&lt;/script&gt;
&lt;progress value={$progress} max="100"&gt;&lt;/progress&gt;</code></pre>
                <p>
                    Emits accumulate and flush together over one long-poll, so many changes cost one
                    round trip. Every window gets its own queue, so a second window does not steal
                    the first one's events. A window that opens later gets no replay of what it
                    missed, so a new window should ask a command for the current state.
                </p>

                <h2 id="domain-events">Domain events, and broadcasting them</h2>
                <p>
                    For events inside the Rust side, where one part announces something and others
                    react without the announcer knowing who they are, there is the
                    <code>Dispatcher</code>: Laravel's <code>Event::dispatch</code> and listeners.
                    Listeners run in registration order, one at a time, and the first error stops
                    the chain and fails the command that dispatched. <code>dispatch_background</code>
                    runs the chain on its own task instead.
                </p>
                <p>
                    <code>App::broadcast</code> is Laravel's <code>ShouldBroadcast</code>: it
                    forwards every dispatched event of a type to the EventBus and declares the
                    channel's type for codegen, so the page gets a typed channel:
                </p>
                <pre><code>#[derive(Clone, serde::Serialize, specta::Type)]
struct NoteArchived { note_id: i64 }

App::new().broadcast::&lt;NoteArchived&gt;("notes:archived");</code></pre>
                <p>
                    The Rust side dispatches a domain event and never mentions the webview. Elyra
                    dispatches some events of its own, too: from a Laravel backend, when an offline
                    write is sent or refused, and for Laravel's notifications. Freddy will listen to
                    all of them in chapters 12 to 14. The details are on the
                    <a href="../events.html">events page</a>.
                </p>""",
    "learned": [
        "<code>App::bind</code>, <code>bind_as</code> for interfaces and <code>bind_lazy</code> for services that need others; resolve with <code>ctx.get</code>.",
        "A provider's <code>register</code> binds, its <code>boot</code> resolves.",
        "Middleware wraps every command; aliases and groups attach it to some, and an unknown name stops the app at startup.",
        "The EventBus pushes values to the page as Svelte stores; the Dispatcher carries domain events inside Rust, and <code>App::broadcast</code> joins the two.",
    ],
    "next": "Freddy gives his notes a database.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 5,
    "title": "A database on the desktop",
    "summary": "One Database for SQLite, MySQL and Postgres, migrations and the schema builder, and #[derive(Model)].",
    "lead": (
        "Freddy's notes need to live somewhere. On the desktop that is usually a SQLite "
        "file next to the app, and Elyra treats it the way Laravel treats any database: "
        "migrations, a schema builder, and an Active Record model."
    ),
    "body": r"""                <h2 id="the-database">One Database</h2>
                <p>
                    The database layer is behind the <code>database</code> feature, so Freddy turns
                    it on in <code>Cargo.toml</code>, on the same tagged dependency as before:
                </p>
                <pre><code>elyra = { git = "https://github.com/kwhorne/elyra-framework", tag = "v0.16.0", features = ["database"] }</code></pre>
                <p>
                    One <code>Database</code> handle spans SQLite, MySQL and Postgres through sqlx,
                    and the URL scheme picks the driver. <code>App::database</code> connects lazily
                    and binds the handle into the container, so commands get it with
                    <code>ctx.get::&lt;Database&gt;()</code>:
                </p>
                <pre><code>use elyra_db::sqlite_url;

let path = elyra::system::paths().data.map(|d| format!("{d}/freddy-notes/notes.db")).unwrap();
App::new().database(sqlite_url(&amp;path))</code></pre>
                <p>
                    That puts the file in the user's data folder. <code>sqlite_url</code> is there
                    for a reason the <a href="../database.html">database page</a> spells out:
                    <code>format!("sqlite://{}", ...)</code> is the obvious thing to write and is
                    wrong on Windows. SQLite also gets WAL mode, a busy timeout and foreign keys on
                    connect, so the window and background jobs can share the file, and the pool is
                    small with a bounded wait, which suits a desktop.
                </p>

                <h2 id="migrations">Migrations</h2>
                <p>
                    Migrations are Elyra's <code>php artisan migrate</code>: SQL files applied in
                    order, tracked in a <code>_elyra_migrations</code> table and grouped into
                    batches, so a rollback undoes the most recent <code>migrate</code> as a unit.
                </p>
                <pre><code>rata make:migration create_notes   # scaffold up + down files
rata migrate                       # apply all pending, as one new batch
rata migrate:status                # list applied and pending
rata migrate:rollback              # undo the most recent batch</code></pre>
                <p>
                    Hand-written SQL differs per driver, so there is a schema builder that renders
                    it for SQLite, MySQL or Postgres. With <code>--rust</code>,
                    <code>make:migration</code> scaffolds a Rust migration that uses it. Freddy's
                    notes table:
                </p>
                <pre><code>Schema::create("notes", |t| {
    t.id();
    t.foreign_id("notebook_id", "notebooks").on_delete_cascade();
    t.string("title");
    t.text("body");
    t.boolean("pinned").default_value("0");
    t.timestamps();
    t.soft_deletes();
    t.index("title");
})</code></pre>
                <p>
                    Rust and SQL migrations share one history table and mix freely. When
                    <code>src/</code> has a Rust migration, <code>rata migrate</code> runs the app in
                    its migrate mode, which knows them; <code>ELYRA_MIGRATE=up cargo run</code> does
                    the same without <code>rata</code>. A seeder fills demo data with
                    <code>ELYRA_SEED=1 cargo run</code>. One portability rule from the
                    <a href="../migrations.html">migrations page</a>: boolean columns are
                    <code>INTEGER</code> 0/1, which is what the schema builder writes and what models
                    expect.
                </p>

                <h2 id="models">Models</h2>
                <p>
                    <code>#[derive(Model)]</code> is Elyra's Eloquent: a thin Active Record layer
                    over the pool, not a second ORM. Freddy's note:
                </p>
                <pre><code>#[derive(Model, serde::Serialize, serde::Deserialize, specta::Type)]
#[model(table = "notes", timestamps, soft_deletes, belongs_to(Notebook, fk = "notebook_id"))]
struct Note {
    #[model(id)] id: i64,
    notebook_id: i64,
    title: String,
    body: String,
    pinned: bool,              // INTEGER 0/1
    created_at: i64,
    updated_at: i64,
    deleted_at: Option&lt;i64&gt;,
}</code></pre>
                <p>
                    Because a model is a plain struct with <code>specta::Type</code>, a command that
                    returns <code>Vec&lt;Note&gt;</code> becomes <code>Promise&lt;Note[]&gt;</code> in
                    <code>bindings.ts</code>. And unknown options in <code>#[model(...)]</code> are
                    compile errors: a misspelt option used to compile into something quietly wrong.
                </p>
                <pre><code>let db = ctx.get::&lt;Database&gt;();
let mut note = Note { id: 0, notebook_id: 1, title: "Train ideas".into(), body: "".into(),
                      pinned: false, created_at: 0, updated_at: 0, deleted_at: None };
note.insert(&amp;db).await?;          // sets note.id and the timestamps
note.pinned = true;
note.save(&amp;db).await?;            // insert when id == 0, otherwise update

let pinned = Note::query().where_eq("pinned", true).order_by_desc("updated_at").get(&amp;db).await?;</code></pre>

                <h2 id="what-a-notes-app-needs">What a notes app needs from it</h2>
                <ul>
                    <li><strong>A search box.</strong> <code>when_some</code> applies a clause only
                        when there is something to filter on, and <code>or_where_like</code> searches
                        several columns at once:
                        <code>Note::query().when_some(term, |q, t| q.or_where_like(&amp;["title", "body"], format!("%{t}%")))</code>.</li>
                    <li><strong>Pages.</strong> <code>paginate(&amp;db, page, 25)</code> returns a
                        <code>Page</code> with <code>data</code>, <code>total</code> and
                        <code>last_page</code>, and a command can return it as is: the page gets
                        <code>Promise&lt;Page&lt;Note&gt;&gt;</code>.</li>
                    <li><strong>A bin.</strong> With <code>soft_deletes</code> every query skips
                        trashed rows; <code>soft_delete</code> and <code>restore</code> go through the
                        builder, and <code>with_trashed</code> reaches them. Note that the instance
                        method <code>note.delete(&amp;db)</code> is a <em>hard</em> delete.</li>
                    <li><strong>Notebooks without N+1.</strong> A relation accessor is one query
                        each, so for a list there is a <code>load_*</code> method that loads them all
                        in one query and returns a map.</li>
                    <li><strong>Test data.</strong> A <code>Factory</code> is a recipe for a valid
                        row: <code>Note::factory().count(50).create(&amp;db)</code>.</li>
                </ul>
                <div class="callout">
                    <strong>What is not there yet.</strong> The <a href="../models.html">models
                    page</a> is candid: <code>group_by</code>/<code>having</code>,
                    <code>first_or_create</code>, <code>upsert</code>, polymorphic relations,
                    <code>has_many_through</code>, model events and observers, and accessors and
                    mutators are not implemented yet. SQLite is fully test-covered; MySQL and
                    Postgres run model CRUD against real servers in CI. For Freddy's notes app,
                    nothing on that list is in the way.
                </div>

                <h2 id="changes">Every write reports its table</h2>
                <p>
                    One last detail sets up chapter 7. Every model-layer write reports its table once
                    it is durable, and inside a transaction the reports wait for the commit. Raw SQL
                    cannot be seen, so after one Freddy calls <code>db.touch("notes")</code>. That
                    stream of changed tables is what lets a list on screen update itself.
                </p>""",
    "learned": [
        "The <code>database</code> feature gives one <code>Database</code> for SQLite, MySQL and Postgres; <code>sqlite_url</code> builds a file URL that also works on Windows.",
        "<code>rata migrate</code> and its friends run SQL or Rust migrations, in batches, with a schema builder for portable DDL.",
        "<code>#[derive(Model)]</code> gives Active Record, a query builder, relations with eager loading, soft deletes, pagination and factories.",
        "What is not implemented yet is listed on the models page, and every write reports its table.",
    ],
    "next": "Freddy generates his screens from the model, with validation that reaches the form.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 6,
    "title": "make:resource, and validation all the way to the form",
    "summary": "Generating a whole slice from a model, the registries, abilities, and errors that land under their field.",
    "lead": (
        "A list to search and page through, a form, a detail view, delete with a "
        "confirmation, and the commands and tests behind them. Freddy writes none of "
        "it by hand: <code>rata make:resource</code> writes it, and the code is his afterwards."
    ),
    "body": r"""                <h2 id="three-tiers">Three tiers</h2>
                <pre><code>rata make:resource Note               # commands, validation, events, tests, for your model
rata make:resource Note --view        # + the Svelte screens
rata make:resource Note --generate &hellip; # + the model, migration, factory, seeder</code></pre>
                <p>
                    The guide calls the result a <em>resource</em>: the vertical slice a desktop app
                    needs for each kind of record. It is not a runtime you configure, but code
                    written into your project, and it is the reference for how things are meant to be
                    done in Elyra.
                </p>

                <h2 id="notebooks">Notebooks, generated</h2>
                <p>
                    Freddy's notebooks are a new table, so he starts with <code>--generate</code>. A
                    field is <code>name:type[:modifier&hellip;][?][=default]</code>:
                </p>
                <pre><code>rata make:resource Notebook --generate name:string:unique 'colour:string?'</code></pre>
                <p>
                    In zsh a field with <code>?</code> needs quotes. <code>make:resource</code> checks
                    that the project has the <code>database</code> feature, <code>serde_json</code>
                    and, for the tests, <code>tokio</code>, and prints the exact lines if one is
                    missing; it does not edit <code>Cargo.toml</code> itself. A parent must be
                    generated before a resource that references it, because the migrations run in the
                    order they were generated.
                </p>

                <h2 id="notes">Notes, from the model he already has</h2>
                <p>
                    For notes Freddy already wrote the model in chapter 5. Without
                    <code>--generate</code>, <code>make:resource</code> finds
                    <code>#[derive(Model)] struct Note</code> under <code>src/</code> and reads it
                    with <code>syn</code>, so the commands, rules and form match its fields. The model
                    must derive <code>Default</code>, <code>Clone</code>, <code>Serialize</code>,
                    <code>Deserialize</code> and <code>specta::Type</code>, and have an
                    <code>i64</code> key; rata says which are missing. Freddy adds
                    <code>Default</code> and <code>Clone</code> and runs it with the screens:
                </p>
                <pre><code>rata make:resource Note --view</code></pre>

                <h2 id="wire-it-once">Wire it once</h2>
                <p>
                    The first resource prints what <code>main.rs</code> needs, and it is the last edit
                    he makes there; every later resource is picked up by the registry:
                </p>
                <pre><code>mod resources;

App::new()
    .database(sqlite_url(&amp;path))
    .commands(resources::commands())
    .migrations(resources::migrations())
    .seeders(resources::seeders())
    .allow_abilities(resources::abilities())</code></pre>
                <p>
                    <code>allow_abilities</code> is deliberate. Each generated command is gated by an
                    ability, <code>notes.view</code>, <code>.create</code>, <code>.update</code>,
                    <code>.delete</code>, and denied until the app grants it: chapter 3's gating,
                    applied for him. Granting fewer makes a read-only app;
                    <code>--no-abilities</code> skips it for a prototype.
                </p>

                <h2 id="what-he-got">What he got</h2>
                <table>
                    <thead><tr><th>Command</th><th>Ability</th><th>What it does</th></tr></thead>
                    <tbody>
                        <tr><td><code>notes_index(query)</code></td><td><code>notes.view</code></td><td><strong>live</strong>: search over the text columns, sort by an allowlisted column, a page of at most 100 rows</td></tr>
                        <tr><td><code>notes_show(id)</code></td><td><code>notes.view</code></td><td><strong>live</strong>: one record, or "not found"</td></tr>
                        <tr><td><code>notes_store(input)</code></td><td><code>notes.create</code></td><td>validate, insert, dispatch <code>NoteCreated</code></td></tr>
                        <tr><td><code>notes_update(id, input)</code></td><td><code>notes.update</code></td><td>validate with the same rules, update, dispatch <code>NoteUpdated</code></td></tr>
                        <tr><td><code>notes_destroy(id)</code></td><td><code>notes.delete</code></td><td>delete, a soft delete since his model has <code>soft_deletes</code>, and dispatch <code>NoteDeleted</code></td></tr>
                    </tbody>
                </table>
                <p>
                    The input type is what a form may set: no key and no timestamps, so a request
                    cannot set them, and its fields are all optional, so a missing one comes back as a
                    validation message instead of a decode error. The events are the hook for
                    everything else, an audit line or a notification, through chapter 4's listeners.
                </p>
                <p>
                    On the Svelte side he got <code>Index.svelte</code>, <code>Form.svelte</code> and
                    <code>Show.svelte</code> with routes and a nav entry: a list that searches once
                    typing pauses, sorts by its headers, pages, and asks before deleting; a form whose
                    controls follow each field's type; and a detail view. Then:
                </p>
                <pre><code>cargo test resources::     # the generated tests, on throwaway SQLite files
rata migrate
rata codegen
rata dev</code></pre>
                <p>
                    The generated tests run the real migrations and cover the whole slice, including
                    validation on create and update, the sort allowlist, the paging cap, the events,
                    that the reads are live, and that every command needs a granted ability.
                </p>

                <h2 id="validation">Validation, all the way to the form</h2>
                <p>
                    The rules live in <code>RULES</code> in the resource's <code>commands.rs</code>,
                    as Laravel-style rule strings, and any of the 44 rules work. In a command of his
                    own, Freddy uses the same validator directly:
                </p>
                <pre><code>Validator::new(&amp;data)
    .rules(&amp;[
        ("title", "required|string|max:200"),
        ("notebook_id", "required|exists:notebooks,id"),
    ])
    .validate_with(&amp;db)
    .await?;</code></pre>
                <p>
                    <code>unique</code> and <code>exists</code> query the database, so they go
                    through the async <code>validate_with</code>; the synchronous
                    <code>validate()</code> panics on a database rule rather than skipping it, and an
                    unknown rule panics too, naming the rule and the field. A misspelt
                    <code>"requried"</code> must not quietly let invalid input through.
                </p>
                <p>
                    On the page, the failure arrives as a typed <code>ValidationError</code> with
                    Laravel's error bag, because the bridge marks the response with its kind; no
                    string sniffing:
                </p>
                <pre><code>import { ValidationError } from "@elyra/runtime";

try {
  await api.notes_store({ title, notebook_id });
} catch (e) {
  if (e instanceof ValidationError) {
    errors = e.errors;          // { title: ["&hellip;"], notebook_id: ["&hellip;"] }
  }
}</code></pre>
                <p>
                    In the generated form that is already done: a failure lands under its field.
                    With a <code>lang/en.json</code>, every label is a translation key, and passing
                    the app's <code>Translator</code> to the validator puts the messages in the user's
                    language. Chapter 11 sends this same error bag the other way: a 422 from Laravel
                    lands in the same form, per field.
                </p>
                <p>
                    The guides are <a href="../resources.html">Resources</a> and
                    <a href="../validation.html">Validation</a>.
                </p>""",
    "learned": [
        "<code>rata make:resource</code> writes commands, tests, events and, with <code>--view</code>, the Svelte screens; <code>--generate</code> adds the model and migration.",
        "On an existing model it reads the struct, which must derive <code>Default</code>, <code>Clone</code>, <code>Serialize</code>, <code>Deserialize</code> and <code>specta::Type</code>.",
        "Each generated command needs an ability, granted with <code>allow_abilities</code>.",
        "Rule strings validate input, database rules go through <code>validate_with</code>, and a <code>ValidationError</code> reaches the form per field.",
    ],
    "next": "Freddy's lists start updating themselves.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 7,
    "title": "Live queries: lists that update themselves",
    "summary": "#[command(live)], how Elyra decides what to re-run, the live.* stores, and when not to.",
    "lead": (
        "Freddy opens his notes in two windows, edits one note in the first, and the "
        "second one changes by itself. He did not write a refresh anywhere. This chapter "
        "is about why."
    ),
    "body": r"""                <h2 id="the-idea">The idea</h2>
                <p>
                    A live query is a command the page <em>subscribes</em> to. Elyra records what it
                    reads, re-runs it when that data changes, and pushes the new result to every
                    window watching. It works because a desktop app's writes happen in the same
                    process as its reads: the model layer sees every insert, update and delete, and
                    knows the table.
                </p>
                <p>
                    Freddy's generated <code>notes_index</code> and <code>notes_show</code> from
                    chapter 6 are already live. His own first live command is a badge in the nav with
                    the number of pinned notes:
                </p>
                <pre><code>#[command(live, can = "notes.view")]
async fn pinned_count(ctx: Ctx) -&gt; elyra::Result&lt;i64&gt; {
    let db = ctx.get::&lt;Database&gt;();
    Ok(Note::query().where_eq("pinned", true).count(&amp;db).await?)
}</code></pre>
                <p>
                    There is nothing live-specific inside it. On the page, <code>rata codegen</code>
                    gave it a store in <code>live.*</code>, next to its <code>api.*</code> call:
                </p>
                <pre><code>&lt;script&gt;
  import { live } from "./bindings";
  const pinned = live.pinned_count();
&lt;/script&gt;
{#if $pinned.value}&lt;span class="badge"&gt;{$pinned.value}&lt;/span&gt;{/if}</code></pre>
                <p>
                    The store's value is a <code>Live&lt;T&gt;</code>: <code>value</code>, the latest
                    result; <code>error</code>, the latest re-run's failure with the previous value
                    kept; and <code>loading</code>, true until the first result. The subscription
                    opens with the store's first listener and closes with its last, and with
                    <code>$derived</code> a changed argument, a new search term say, opens a new
                    subscription and closes the old one.
                </p>

                <h2 id="what-to-re-run">How Elyra decides what to re-run</h2>
                <ol>
                    <li><strong>Reads are recorded.</strong> While a live command runs, every
                        model-layer read adds its table to the command's read set: a query, a join, a
                        relation, a pivot. Nothing to declare.</li>
                    <li><strong>Writes are reported.</strong> Every model-layer write reports its
                        table once it is durable, which is chapter 5's last section. Inside a
                        transaction the reports go out on commit, and a rollback sends none.</li>
                    <li><strong>What changed is matched against what was read.</strong> Only
                        subscriptions whose read set contains the table re-run.</li>
                </ol>
                <p>
                    Then a burst of writes is coalesced into one re-run per subscription per batch
                    window, one frame of 16&nbsp;ms by default. A result byte-identical to the last
                    one pushed is not pushed. The push goes only to the subscribing window, and
                    re-runs go one round at a time, so an older result never arrives after a newer
                    one.
                </p>
                <p>
                    Freddy pins a note in one window. The <code>notes</code> table is reported, both
                    windows' <code>notes_index</code> and the badge re-run, and the badge's new number
                    is pushed. Had he only edited a note's body, the count would have come out the
                    same, and nothing would have been pushed for the badge.
                </p>

                <h2 id="what-it-cannot-see">What the model layer cannot see</h2>
                <p>
                    Raw SQL, another process, a file, a cache or a setting: those he declares and
                    reports himself. Keys are strings; a table's key is its name, and anything else is
                    his to name:
                </p>
                <pre><code>#[command(live)]
async fn word_count(ctx: Ctx) -&gt; elyra::Result&lt;i64&gt; {
    ctx.depends_on("notes");            // read with raw SQL below
    ctx.depends_on("settings:language");
    // ...
}

// after a raw write elsewhere
db.touch("notes");                      // or ctx.invalidate("notes")</code></pre>

                <h2 id="rules">The rules</h2>
                <ul>
                    <li><strong>A live command only reads.</strong> Since Elyra re-runs it whenever
                        its data changes, a write inside it would re-trigger itself, so during a live
                        run a model-layer write fails. The same command called once through
                        <code>api.*</code> is an ordinary call.</li>
                    <li><strong>Opt-in</strong>: only <code>#[command(live)]</code> can be subscribed
                        to, and subscribing goes through the same guard as any call, including the
                        command's ability.</li>
                    <li><strong>Bounded</strong>: a window holds at most 64 subscriptions, and a window
                        that goes away takes its subscriptions with it.</li>
                    <li><strong>It needs the <code>database</code> feature</strong>, because the change
                        tracking lives in the database layer.</li>
                </ul>

                <h2 id="testing-it">Testing it</h2>
                <p>
                    A test can hold a live handle and assert that a write pushes, rather than
                    re-fetching. Freddy's version of the guide's example:
                </p>
                <pre><code>let mut pinned = app.live::&lt;i64&gt;("pinned_count", ()).await;
assert_eq!(pinned.value(), 0);

app.invoke_ok::&lt;Note&gt;("notes_store", (input,)).await;     // a pinned note
assert_eq!(pinned.next().await, 1);                          // pushed, not re-fetched</code></pre>

                <h2 id="when-not-to">When not to</h2>
                <p>
                    A live query re-runs the whole command. That is cheap for what a desktop app
                    shows, a page of rows or a count, and it is what lists, details, dashboards and
                    badges want. For a stream of fine-grained events such as the export's progress,
                    chapter 4's channel is the tool; for an expensive report, an <code>api.*</code>
                    call the user refreshes.
                </p>
                <p>
                    A live command can also be watched by an AI agent, which chapter 10 shows, and
                    chapter 11 makes live queries follow a Laravel backend's data. The guide is
                    <a href="../live-queries.html">Live queries</a>.
                </p>""",
    "learned": [
        "<code>#[command(live)]</code> makes a command subscribable; <code>live.*</code> in <code>bindings.ts</code> gives a store with <code>value</code>, <code>error</code> and <code>loading</code>.",
        "Reads are recorded and writes reported per table; only matching subscriptions re-run, coalesced, and an unchanged result is not pushed.",
        "Raw SQL and outside data are declared with <code>ctx.depends_on</code> and reported with <code>db.touch</code> or <code>ctx.invalidate</code>.",
        "A live command only reads, and a window holds at most 64 subscriptions.",
    ],
    "next": "Freddy makes the app feel like it belongs on the desktop.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 8,
    "title": "Being a real desktop app",
    "summary": "Windows, the menu, the tray, global shortcuts, one instance with deep links, autostart, dialogs and the clipboard.",
    "lead": (
        "Freddy wants a quick-note window behind a shortcut, a File menu, an icon in "
        "the menu bar, and an app that opens a note from a link. None of it is a web "
        "page's business, and all of it is a few builder calls."
    ),
    "body": r"""                <h2 id="features">Features first</h2>
                <p>
                    Most of this chapter is behind Cargo features, so an app only compiles in what
                    it uses. Freddy adds them to the same dependency line:
                </p>
                <pre><code>elyra = { git = "https://github.com/kwhorne/elyra-framework", tag = "v0.16.0",
          features = ["database", "tray", "shortcuts", "autostart", "system"] }</code></pre>
                <p>
                    Windows, the menu, single-instance and deep links are core and need no feature.
                    The framework runs on macOS, Windows and Linux; where a platform behaves
                    differently, the guide says so, and so does this chapter.
                </p>

                <h2 id="windows">Windows</h2>
                <p>
                    The primary window is configured on <code>App</code>; more can be declared at
                    startup or opened at runtime, and all of them share one event loop, one protocol
                    handler and one event bus:
                </p>
                <pre><code>App::new()
    .title("Freddy Notes")
    .size(900.0, 640.0)
    .min_size(420.0, 480.0)
    .persist_window_state()      // size, position and maximized, restored next launch
    .run();

#[command]
async fn open_quick_note(ctx: Ctx) {
    ctx.get::&lt;Windows&gt;().open(
        WindowConfig::new("quick").title("Quick note").size(420.0, 260.0)
            .always_on_top(true).path("quick"),
    );
}</code></pre>
                <p>
                    <code>path</code> opens a route of the Svelte app, so the quick-note window is
                    just another page. From the page, <code>appWindow</code> in
                    <code>@elyra/runtime</code> minimizes, sets the title, centers, closes and reports
                    size and focus; <code>onFileDrop</code> hands over the files
                    dropped on the window, which is how Freddy imports Markdown files. The app exits
                    when the last window closes.
                </p>

                <h2 id="menu">The menu</h2>
                <pre><code>App::new().menu(
    Menu::new().submenu(
        Submenu::new("File")
            .item_accel("file.new", "New note", "CmdOrCtrl+N")
            .separator()
            .item("file.export", "Export&hellip;"),
    ),
)</code></pre>
                <p>
                    A click emits <code>elyra:menu</code> with the item's id, and the page handles it
                    with <code>onMenu</code>. On macOS the submenus join the application menu bar next
                    to the built-in app and Edit menus, the Edit menu being what makes &#8984;C and
                    &#8984;V reach the webview; on Windows and Linux they become a per-window menu bar
                    with a <em>Help &rarr; About</em> item.
                </p>

                <h2 id="tray-and-shortcuts">The tray and a global shortcut</h2>
                <pre><code>App::new()
    .tray(TrayConfig::new().tooltip("Freddy Notes")
        .item("quick", "Quick note").separator().quit("Quit"))
    .global_shortcut("CmdOrCtrl+Shift+N")</code></pre>
                <p>
                    A tray item's id arrives on the <code>"tray"</code> channel from chapter 4, and a
                    global shortcut, which fires even when the app is not focused, arrives through
                    <code>onShortcut</code> with its accelerator string. Freddy sends both to
                    <code>open_quick_note</code>. A shortcut that is invalid or already taken is
                    logged and skipped, and the others still register. The tray generates a plain
                    icon, so no image asset is needed to start.
                </p>

                <h2 id="one-instance">One instance, and links that open a note</h2>
                <pre><code>App::new().single_instance().deep_link("freddynotes").run()?;</code></pre>
                <p>
                    With <code>single_instance</code>, a second launch hands its command line to the
                    first process and exits; the first raises its window and gets the payload on
                    <code>elyra:second-instance</code>. The handshake between them proves a random
                    per-install token with an HMAC instead of sending it, so another local process
                    cannot inject a fake deep link. With <code>deep_link</code>, Freddy reads the URL
                    the app was launched with through <code>deepLink.initial()</code> and later ones
                    through <code>onDeepLink</code>, and opens the note named in
                    <code>freddynotes://&hellip;</code>. Registering the scheme with the OS is the
                    bundle's job: without <code>deep_link = "freddynotes"</code> under
                    <code>[bundle]</code> in <code>elyra.toml</code>, the scheme never reaches the app,
                    which chapter 15 sets up.
                </p>

                <h2 id="autostart">Starting at login</h2>
                <pre><code>import { autostart } from "@elyra/runtime";
if (!(await autostart.isEnabled())) await autostart.enable();</code></pre>
                <p>
                    That is a LaunchAgent on macOS, the registry's Run key on Windows and a
                    <code>.desktop</code> entry on Linux. Freddy puts it behind a checkbox in his
                    settings, since starting at login is the user's decision.
                </p>

                <h2 id="system">Dialogs, the clipboard and the OS</h2>
                <p>
                    The <code>system</code> feature adds what almost every app needs, driven from the
                    page with no Rust wiring: <code>dialog.open</code> and <code>dialog.save</code>
                    for files and folders, <code>clipboard</code>, <code>notify</code>,
                    <code>paths()</code> for the standard folders, and <code>shell.openExternal</code>.
                    That last one is policy-gated, because "open with the default app" must not become
                    "run this program": only <code>http</code>, <code>https</code> and
                    <code>mailto</code> by default, no <code>file:</code> URLs, and nothing executable.
                    A blocked call rejects with a message instead of doing nothing.
                </p>
                <div class="callout">
                    <strong>Two things want a bundled app.</strong> On macOS, notifications are only
                    delivered from a bundled app with a bundle identifier, and autostart may not
                    persist for a bare <code>cargo run</code> binary. If they seem not to work while
                    Freddy develops, that is why; chapter 15 bundles the app.
                </div>
                <p>
                    The guides are <a href="../windows.html">Windows</a>,
                    <a href="../menu.html">Application menu</a>, <a href="../tray.html">System
                    tray</a>, <a href="../shortcuts.html">Global shortcuts</a>,
                    <a href="../single-instance.html">Single-instance &amp; deep-linking</a>,
                    <a href="../autostart.html">Autostart</a> and
                    <a href="../system.html">System integration</a>.
                </p>""",
    "learned": [
        "Windows are configured on <code>App</code> or opened at runtime with <code>Windows::open</code>, each able to open a route of the Svelte app.",
        "The menu, the tray and global shortcuts arrive on the page as events, and the platforms render menus differently.",
        "<code>single_instance</code> and <code>deep_link</code> make one app that opens links, with an authenticated handshake.",
        "The <code>system</code> feature gives dialogs, the clipboard, notifications and paths, and <code>openExternal</code> is policy-gated.",
        "On macOS, notifications and autostart want a bundled app.",
    ],
    "next": "Freddy stores settings and secrets, and puts work in the background.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 9,
    "title": "Settings, secrets and work in the background",
    "summary": "The settings store, the keychain, a jailed storage disk, the cache, the queue and the scheduler.",
    "lead": (
        "Freddy's app now remembers his theme, keeps a token where tokens belong, exports "
        "without freezing the window and empties the bin on a schedule. Each of these is "
        "a Laravel facade he already knows, scaled down to one process on one machine."
    ),
    "body": r"""                <h2 id="settings">Settings: the store</h2>
                <p>
                    For settings and light state there is a key-value store, always available, no
                    feature flag. Values are any JSON, saved to a <code>settings.json</code> in the OS
                    config directory:
                </p>
                <pre><code>import { store } from "@elyra/runtime";

await store.set("theme", "dark");
await store.set("lastNotebook", 3);
const theme = await store.get("theme");   // "dark" | null</code></pre>
                <p>
                    Writes are atomic, through a temporary file and a rename, with the previous good
                    version kept as <code>settings.json.bak</code> and used if the main file cannot be
                    parsed. A store set on every keystroke does not write the file per keystroke,
                    because flushes are debounced. <code>store.clear()</code> from the page needs a
                    capability that is opt-in. Notes themselves stay in the database: the store is for
                    settings.
                </p>

                <h2 id="secrets">Secrets: the keychain</h2>
                <p>
                    A token does not belong in a clear-text settings file. With the
                    <code>secrets</code> feature, credentials go to the macOS Keychain, the Windows
                    Credential Manager or the Linux Secret Service:
                </p>
                <pre><code>App::new().provider(SecretsProvider::for_app("Freddy Notes"))

// in a #[command]
let secrets = ctx.get::&lt;Secrets&gt;();
secrets.set("sync_token", &amp;token)?;
let token = secrets.get("sync_token")?;   // Option&lt;Secret&gt;</code></pre>
                <p>
                    A read returns a <code>Secret</code>, not a <code>String</code>: it is used like a
                    string, prints <code>Secret(***)</code> in a stray log line, and wipes its bytes
                    when dropped. There is deliberately no route to secrets from the page, because a
                    value any script in the webview can read is not a secret: read it in a command and
                    return only what the UI needs.
                </p>
                <div class="callout">
                    <strong>Not for AI provider keys.</strong> The <a href="../secrets.html">secrets
                    page</a> is blunt about it: a key inside a distributed desktop binary is a key your
                    users have. Keep provider keys on a server and use <code>Secrets</code> for the
                    user's own credentials. Chapter 10 comes back to this.
                </div>

                <h2 id="storage-and-cache">A disk and a cache</h2>
                <p>
                    The storage facade is Laravel's <code>Storage::</code> over a local directory,
                    with every path jailed to its root, so <code>..</code> and absolute paths are
                    refused. Freddy roots it in his app's data folder, and his exports go to
                    <code>exports/</code> on it. The cache is <code>Cache::</code> in one process,
                    TTL-aware, with <code>remember</code>, <code>add</code>, counters and the rest,
                    which suits the word counts on his dashboard:
                </p>
                <pre><code>App::new()
    .provider(StorageProvider::at(&amp;data_dir))
    .provider(CacheProvider);

let stats = ctx.get::&lt;Cache&gt;().remember("stats", Some(Duration::from_secs(60)), || compute_stats());</code></pre>

                <h2 id="the-queue">The queue</h2>
                <p>
                    Exporting a thousand notes should not happen on the UI's time. The queue is
                    Laravel's <code>Queue::</code> for one process: push a named job, register a
                    handler in a provider's <code>boot</code>, and it runs on a background task.
                </p>
                <pre><code>struct Jobs;
impl Provider for Jobs {
    fn boot(&amp;self, ctx: &amp;Ctx) {
        ctx.get::&lt;Queue&gt;().on("export_notes", |payload| async move {
            // write the files to the storage disk
            Ok(())
        });
    }
}

App::new().provider(QueueProvider::with_workers(2).durable()).provider(Jobs);</code></pre>
                <p>
                    In memory, jobs are lost on exit; <code>durable()</code> keeps every job in the
                    app's database until it succeeds or finally fails, so an export queued before Freddy
                    quits runs at the next launch, with no migration to write. A job can have attempts,
                    a backoff and a timeout, failed jobs are listed and can be retried, and the queue is
                    bounded, so <code>push</code> answers <code>false</code> rather than growing until
                    the process dies. The page can push jobs too and watches their status through
                    <code>onQueue</code>; the job code itself stays in Rust, as Laravel's jobs stay on
                    the server.
                </p>

                <h2 id="the-scheduler">The scheduler</h2>
                <pre><code>let sched = ctx.get::&lt;Scheduler&gt;();
sched.daily_at("03:00", "empty_bin", || async { /* force-delete notes trashed 30 days ago */ });</code></pre>
                <p>
                    The scheduler is Laravel's <code>Schedule</code>, running for the life of the app.
                    One detail catches Laravel developers: intervals are measured from app start, so
                    <code>daily()</code> means every 24 hours, not at midnight. For a time of day, as
                    Freddy wants, there are clock jobs such as <code>daily_at</code>,
                    <code>weekdays_at</code> and <code>weekly_on</code>.
                </p>
                <p>
                    The guides are <a href="../store.html">Settings store</a>,
                    <a href="../secrets.html">Secrets</a>, <a href="../storage.html">Storage</a>,
                    <a href="../cache.html">Cache</a>, <a href="../queue.html">Queue</a> and
                    <a href="../scheduler.html">Scheduler</a>.
                </p>""",
    "learned": [
        "The store keeps settings as JSON with atomic writes and a backup; the database keeps data.",
        "<code>Secrets</code> uses the OS keychain, returns a <code>Secret</code> that cannot leak through a log, and is never reachable from the page.",
        "AI provider keys do not belong in a desktop binary at all.",
        "The queue runs jobs in the background, durably if asked; the scheduler's intervals count from app start, and clock jobs run at a time of day.",
    ],
    "next": "Freddy adds AI to the app, and lets AI agents into it.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 10,
    "title": "AI, MCP, and agents that know the framework",
    "summary": "The AI SDK, exposing commands to AI agents with confirmation, and rata ai:install for the agent writing the code.",
    "lead": (
        "AI shows up three times in Freddy's week: inside his app, suggesting tags; "
        "outside it, as Claude adding a note for him; and in his editor, as the coding "
        "agent writing the app. Elyra Framework has something for each."
    ),
    "body": r"""                <h2 id="ai-in-the-app">AI in the app</h2>
                <p>
                    The AI SDK is a Laravel-inspired API for agents, tools, structured output,
                    images, audio and embeddings over Anthropic and OpenAI, behind the
                    <code>ai</code> feature. It runs in the Rust backend, so a key never reaches the
                    page. With <code>AiProvider</code>, a command resolves the client:
                </p>
                <pre><code>#[derive(serde::Deserialize, JsonSchema)]
struct TagSuggestion { tags: Vec&lt;String&gt; }

#[command]
async fn suggest_tags(ctx: Ctx, body: String) -&gt; Result&lt;TagSuggestion, String&gt; {
    ctx.get::&lt;Ai&gt;().chat()
        .instructions("Suggest at most three short tags for this note.")
        .prompt_as(body)
        .await
        .map_err(|e| e.to_string())
}</code></pre>
                <p>
                    <code>prompt_as</code> forces the model to answer with JSON matching the struct
                    and deserializes it; flat structs work best. For a summary that paints as it is
                    written, <code>stream</code> yields deltas, and the guide's pattern is to emit each
                    one on the event bus and subscribe in Svelte, chapter 4's channel again. Retries
                    are on by default, failover can try the other provider, and plain prompts can be
                    cached.
                </p>
                <div class="callout">
                    <strong>Where the key lives.</strong> The SDK reads
                    <code>ANTHROPIC_API_KEY</code> and <code>OPENAI_API_KEY</code> from the
                    environment, which is fine on Freddy's own machine. Chapter 9's warning stands for
                    an app he gives to others: a key in a desktop binary is a key its users have. The
                    SDK also reads <code>ANTHROPIC_BASE_URL</code> and <code>OPENAI_BASE_URL</code>,
                    the overrides for a proxy or gateway, which is where a shared key belongs. And the
                    guide is honest about testing: the live provider calls only run when
                    <code>ELYRA_AI_LIVE=1</code> and a key are set, and are skipped in CI.
                </div>

                <h2 id="the-app-as-mcp">The app as an MCP server</h2>
                <p>
                    Turn it around: an AI agent in Claude, Cursor or VS Code can call the app's
                    commands as tools, through the same middleware and validation as the page.
                    Nothing is exposed until Freddy says so:
                </p>
                <pre><code>App::new()
    .commands(resources::commands())
    .mcp(Mcp::new()
        .allow_abilities(["notes.view", "notes.create"])
        .allow_ability("notes.delete")
        .confirm("notes.delete"))</code></pre>
                <p>
                    A command is a tool when its ability is granted here, a grant separate from the
                    page's. Its <code>///</code> doc comment is the tool's description, so Freddy writes
                    those for a model. A live command is marked read-only, and one that can run without
                    arguments is also a resource the agent can read and subscribe to, told when the
                    result changes, the way a window is.
                </p>
                <p>
                    <code>confirm</code> makes the server ask the user before such a call runs, in the
                    MCP client, with the tool, the first line of its description and the arguments. The
                    server enforces it: a client that cannot ask gets an error and the tool does not
                    run, and so does a decline or no answer within 10 minutes. Every tool takes at most
                    60 calls a minute by default, a stop for an agent caught in a loop.
                </p>
                <pre><code>rata mcp inspect      # what the agent will see
rata mcp install      # the config that connects Claude to this app</code></pre>
                <p>
                    The client launches <code>freddy-notes --mcp</code>. If the app is open, the agent
                    works inside it over a private local endpoint, and the windows update as it adds a
                    note; if not, the app serves MCP headless. On the page, <code>onAgent</code> shows
                    a toast when a call starts, and in middleware a request's origin is
                    <code>Origin::Agent</code>, so Freddy's audit line from chapter 4 can say an agent
                    did it.
                </p>

                <h2 id="agents-that-know">Agents that know the framework</h2>
                <p>
                    The third agent is the one writing Freddy's code. Since 0.16.0 it does not have to
                    guess at Tauri's or Laravel's API:
                </p>
                <pre><code>rata ai:install       # guidelines, skills, and the dev MCP server
rata ai:update        # refresh them after upgrading rata</code></pre>
                <p>
                    <code>ai:install</code> writes the framework's guidelines as a block in
                    <code>AGENTS.md</code>, and in <code>CLAUDE.md</code> unless it imports
                    <code>AGENTS.md</code>; five skills in <code>.agents/skills</code> and
                    <code>.claude/skills</code>, loaded only when they apply, for commands, models and
                    migrations, resources, live queries and the Laravel backend; and an
                    <code>elyra-framework</code> server in <code>.mcp.json</code> next to the servers
                    already there. Only its own block and skills are rewritten, so Freddy's notes in
                    <code>AGENTS.md</code> survive.
                </p>
                <p>
                    That server is <code>rata mcp dev</code>, and it tells the coding agent what the app
                    is: every command with its abilities and middleware, the models, the database
                    schema, the migrations applied or pending, the newest error in the app's log, and
                    the guides for this version, searched. Every tool only reads, a SQLite database is
                    opened read-only, and the code is read with <code>syn</code>, so it needs no build.
                </p>
                <p>
                    The difference between the two servers is worth keeping straight. The app's own MCP
                    server, from the section above, is the app's commands for an agent <em>using</em>
                    the app. <code>rata mcp dev</code> is the project for an agent <em>writing</em> it.
                    The guides are <a href="../ai.html">AI SDK</a>, <a href="../mcp.html">AI agents
                    (MCP)</a> and <a href="../ai-agents.html">Coding agents</a>.
                </p>""",
    "learned": [
        "The AI SDK gives chat, structured output, streaming, retries and failover in Rust; keys come from the environment, and a shared key belongs behind a proxy.",
        "<code>.mcp(Mcp::new()...)</code> makes granted commands tools for AI agents, with server-enforced confirmation and rate limits.",
        "<code>rata ai:install</code> gives the coding agent the framework's guidelines, skills and <code>rata mcp dev</code>, which only reads.",
    ],
    "next": "Freddy connects the app to his Laravel backend.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 11,
    "title": "A Laravel backend",
    "summary": "Backend and Http, the three Sanctum routes, Laravel's answers as kinds, signing in with the token in the keychain, and make:resource --backend.",
    "lead": (
        "The notes Freddy built so far are his own, on his Mac. His team's notes live in "
        "the Laravel app from <em>Freddy the Notetaker</em>. From here on, the desktop app "
        "is that app's desktop half: same users, same API, same validation."
    ),
    "body": r"""                <h2 id="the-idea">The idea</h2>
                <p>
                    In the guide's words, an Elyra app can be <em>the desktop half of a product whose
                    web half is Laravel</em>. It signs the user in with a Sanctum token, calls the same
                    API as the web, and shows Laravel's validation messages in its forms as if they
                    were its own. Freddy's private notebook from chapters 5 to 7 stays in the local
                    database; his team's notes come from the server. The feature is
                    <code>backend</code>:
                </p>
                <pre><code>elyra = { git = "https://github.com/kwhorne/elyra-framework", tag = "v0.16.0",
          features = ["database", "backend"] }</code></pre>
                <pre><code>App::new().backend(Backend::new("https://notes.example.com"))</code></pre>

                <h2 id="the-laravel-side">The Laravel side</h2>
                <p>
                    On the web app, Freddy needs Sanctum and three routes in
                    <code>routes/api.php</code>: one that issues a token, one that revokes it and one
                    that returns the user. The guide gives them in full:
                </p>
                <pre><code>php artisan install:api      # Sanctum; then add HasApiTokens to App\Models\User</code></pre>
                <pre><code>Route::post('/sanctum/token', function (Request $request) {
    $request-&gt;validate(['email' =&gt; 'required|email', 'password' =&gt; 'required', 'device_name' =&gt; 'required']);
    $user = User::where('email', $request-&gt;email)-&gt;first();
    if (! $user || ! Hash::check($request-&gt;password, $user-&gt;password)) {
        throw ValidationException::withMessages(['email' =&gt; ['The provided credentials are incorrect.']]);
    }
    return $user-&gt;createToken($request-&gt;device_name)-&gt;plainTextToken;
});

Route::middleware('auth:sanctum')-&gt;group(function () {
    Route::delete('/sanctum/token', fn (Request $r) =&gt; $r-&gt;user()-&gt;currentAccessToken()-&gt;delete());
    Route::get('/user', fn (Request $r) =&gt; $r-&gt;user());
    Route::apiResource('notes', NoteController::class);
});</code></pre>
                <p>
                    Different paths are fine: <code>token_route</code>, <code>revoke_route</code> and
                    <code>user_route</code> on <code>Backend</code>. Since 0.16.0 the repository also
                    has a Composer package, <code>elyracode/elyra-backend</code>, with these routes as
                    <code>Route::elyraAuth()</code> and the middleware later chapters need; the 0.16.0
                    changelog says it is not published yet, so this course writes the routes by hand.
                </p>

                <h2 id="signing-in">Signing in</h2>
                <pre><code>import { auth, onSignedOut, ValidationError } from "@elyra/runtime";

try {
  await auth.signIn(email, password);
} catch (e) {
  if (e instanceof ValidationError) errors = e.errors;   // { email: ["&hellip;"] }
}
$auth;                                                    // { signedIn, user, reason }
onSignedOut((reason) =&gt; reason === "expired" &amp;&amp; toast("Please sign in again"));</code></pre>
                <p>
                    Three things make this safe, and they are chapter 1's untrusted page again:
                </p>
                <ul>
                    <li><strong>The token lives in the OS keychain only</strong>, never in the webview
                        or a file, and is restored when the app starts. With no keychain, sign-in fails
                        and the token the server just issued is revoked: there is no plain-text
                        fallback.</li>
                    <li><strong>The device is named after the machine and the app</strong>, so Freddy
                        recognizes it among his tokens on the web.</li>
                    <li><strong>A 401 from any request signs the user out</strong>, and the page hears
                        <code>expired</code>.</li>
                </ul>

                <h2 id="calling-the-api">Calling the API</h2>
                <pre><code>let backend = ctx.get::&lt;Backend&gt;();
let page: Page&lt;TeamNote&gt; = backend.get("/api/notes").query(&amp;query).json().await?;
let note: TeamNote       = backend.get(format!("/api/notes/{id}")).resource().await?;
let created: TeamNote    = backend.post("/api/notes").body(&amp;input).resource().await?;</code></pre>
                <p>
                    The token goes as <code>Bearer</code>, and every request asks for JSON, which is
                    what makes Laravel answer a failed validation with a 422 instead of a redirect.
                    <code>.resource()</code> unwraps an API Resource's <code>data</code>, and
                    <code>Page&lt;T&gt;</code> reads both of Laravel's paginators. Under it is
                    <code>Http</code>, Laravel's HTTP facade in Rust: a 30-second timeout by default,
                    retries for idempotent methods, HTTPS only except to loopback, and a redirect to
                    another host does not carry the token.
                </p>

                <h2 id="answers">Laravel's answers, as kinds</h2>
                <table>
                    <thead><tr><th>Laravel</th><th>The page sees</th></tr></thead>
                    <tbody>
                        <tr><td><code>422 { errors }</code></td><td>a <code>ValidationError</code>, per field, as from a local command</td></tr>
                        <tr><td><code>401</code>, <code>419</code></td><td>kind <code>unauthenticated</code>; the token is forgotten</td></tr>
                        <tr><td><code>403 { message }</code></td><td>kind <code>denied</code>, with Laravel's message</td></tr>
                        <tr><td><code>404</code></td><td>kind <code>not-found</code></td></tr>
                        <tr><td><code>409</code>, <code>412</code></td><td>kind <code>conflict</code> (chapter 13)</td></tr>
                        <tr><td><code>429</code> + <code>Retry-After</code></td><td>kind <code>too-many-requests</code>, saying when</td></tr>
                        <tr><td><code>5xx</code></td><td>kind <code>server</code>, without the error page</td></tr>
                        <tr><td>no answer</td><td>kind <code>offline</code></td></tr>
                    </tbody>
                </table>
                <p>
                    <code>?</code> in a command turns each into the right error, so Freddy's UI can say
                    "you're offline" instead of showing an error, and a 422 from his Laravel
                    <code>StoreNoteRequest</code> lands under the field in the desktop form, just as
                    chapter 6's local validation did.
                </p>

                <h2 id="live-over-the-api">Live queries over the API</h2>
                <p>
                    Chapter 7 carries over. Inside a live command a <code>GET</code> depends on its
                    resource, the path without the query and trailing ids, so
                    <code>/api/notes?page=2</code> and <code>/api/notes/12</code> are both the notes
                    resource, and a successful write to it re-runs them. That covers what this app
                    writes. What a colleague changes on the web is chapter 12.
                </p>

                <h2 id="generate-it">Or generate all of it</h2>
                <pre><code>rata make:resource TeamNote --backend /api/notes --generate title:string body:text 'pinned:bool?'</code></pre>
                <p>
                    That is chapter 6's resource, with the same screens, types, abilities and events,
                    except that its commands call the API. There is no migration or seeder, because the
                    table is the server's, and validation is the server's too. rata prints what the
                    Laravel side needs, and the tests run against an <code>HttpFake</code>. For now the
                    project needs the <code>database</code> feature as well, because the resource
                    registry also lists migrations.
                </p>
                <p>
                    The guides are <a href="../backend.html">A Laravel backend</a> and
                    <a href="../http.html">HTTP client</a>.
                </p>""",
    "learned": [
        "<code>Backend</code> calls a Laravel API with the user's Sanctum token; <code>Http</code> under it is Laravel's HTTP facade in Rust.",
        "Three routes on the Laravel side; <code>elyracode/elyra-backend</code> has them too, but is not published yet.",
        "The token lives in the keychain only, and a 401 anywhere signs the user out.",
        "Laravel's answers arrive as kinds, and a 422 lands in the form per field.",
        "<code>make:resource --backend</code> generates a resource whose commands call the API.",
    ],
    "next": "Changes made on the web show up in Freddy's open windows.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 12,
    "title": "Changes on the web, in the open windows",
    "summary": "Listening to Laravel's broadcasting like Echo does, following a channel, and what each event does.",
    "lead": (
        "Freddy's colleague edits a team note in the browser. On Freddy's desktop the "
        "open list changes a moment later, without a refresh and without polling. Laravel "
        "announced the change; Elyra listened, the way Echo does in a browser."
    ),
    "body": r"""                <h2 id="follow">Follow a channel</h2>
                <pre><code>App::new().backend(
    Backend::new("https://notes.example.com")
        .broadcasting(Broadcasting::pusher("app-key"))   // REVERB_APP_KEY
        .follow("notes", "/api/notes"),                  // private-notes: re-run what read /api/notes
)</code></pre>
                <p>
                    <code>follow</code> ties a Laravel channel to a resource. When an event arrives on
                    <code>private-notes</code>, every live query that read <code>/api/notes</code>
                    re-runs, coalesced, and is pushed only if the result changed, which is chapter 7's
                    machinery fed from the network. A resource generated with <code>--backend</code>
                    already follows its channel; <code>follow_all(resources::follows())</code> wires
                    them all at once.
                </p>

                <h2 id="the-connection">The connection</h2>
                <ul>
                    <li><strong>The protocol</strong> is Pusher's, which Askr (with
                        <code>--pusher</code>), Reverb and Pusher speak. It goes to the backend's own
                        host, and <code>.host()</code>, <code>.port()</code> and <code>.scheme()</code>
                        point it at a Reverb on its own port. It is <code>wss</code> only, except to
                        loopback.</li>
                    <li><strong>It opens once the user is signed in</strong> and closes on sign-out. A
                        dropped connection reconnects, backing off from one second up to thirty, and
                        then re-runs everything followed, because events sent in the meantime are
                        lost.</li>
                    <li><strong>Private channels are authorized by the backend</strong>, as the
                        signed-in user, at <code>/api/broadcasting/auth</code>. The app secret stays on
                        the server.</li>
                    <li><strong>Laravel's <code>-&gt;toOthers()</code> works</strong>: while connected,
                        <code>Backend</code> sends <code>X-Socket-ID</code>, so Laravel can skip the app
                        whose own write already re-ran its queries.</li>
                </ul>

                <h2 id="three-things">What each event does</h2>
                <ol>
                    <li>It <strong>re-runs the live queries</strong> that read the followed resource.</li>
                    <li>It is a <strong>domain event</strong>, <code>BackendEvent { channel, event, data }</code>,
                        so chapter 4's <code>App::listen</code> can log it or raise a notification.</li>
                    <li>It <strong>reaches the windows</strong>:</li>
                </ol>
                <pre><code>import { onBackendEvent, broadcasting } from "@elyra/runtime";

onBackendEvent("notes", "NoteUpdated", (data) =&gt; toast(`${data.model.title} was updated`));
// $broadcasting.state: "connecting" | "connected" | "offline"</code></pre>
                <p>
                    Freddy shows a small dot in the corner from <code>$broadcasting.state</code>. One
                    caution from the guide: the data is the server's, untrusted like any input.
                    Re-running a query does not read it at all, only the fact that something changed.
                    Presence channels are not supported yet.
                </p>

                <h2 id="laravel-side">On the Laravel side</h2>
                <pre><code>php artisan install:broadcasting --reverb   # or Askr: askr serve --pusher</code></pre>
                <pre><code>// app/Models/Note.php
class Note extends Model
{
    use BroadcastsEvents;

    public function broadcastOn(string $event): array
    {
        return [new PrivateChannel('notes')];
    }
}

// routes/channels.php
Broadcast::channel('notes', fn ($user) =&gt; true);

// bootstrap/app.php: channel auth under /api, with the Sanctum token
-&gt;withBroadcasting(__DIR__.'/../routes/channels.php', [
    'prefix' =&gt; 'api', 'middleware' =&gt; ['api', 'auth:sanctum'],
])</code></pre>
                <p>
                    Broadcast events are queued by default, so the Laravel app needs a queue worker, or
                    <code>QUEUE_CONNECTION=sync</code>. Freddy's channel rule lets every signed-in user
                    listen, because the whole team shares the notes; a stricter app returns a real
                    check there. The guide is the broadcasting section of
                    <a href="../backend.html">A Laravel backend</a>, and the design is
                    <a href="../0006-laravel-broadcasting.html">RFC 0006</a>.
                </p>""",
    "learned": [
        "<code>.broadcasting(Broadcasting::pusher(key)).follow(channel, path)</code> listens like Echo, over the protocol Askr, Reverb and Pusher speak.",
        "The connection follows sign-in, reconnects with backoff and re-runs everything followed afterwards.",
        "Each event re-runs live queries, is a <code>BackendEvent</code> in Rust, and reaches the page through <code>onBackendEvent</code>.",
        "The event's data is untrusted, and presence channels are not supported yet.",
    ],
    "next": "Freddy gets on a train with no network.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 13,
    "title": "Working offline",
    "summary": "The last answer per user, the outbox that sends each write once, the connection and outbox stores, and conflicts.",
    "lead": (
        "On the train the network comes and goes. Freddy keeps reading his team's notes "
        "and writes two new ones. When the network is back, both are sent, once each, and "
        "an edit that clashed with a colleague's waits for him to decide."
    ),
    "body": r"""                <h2 id="turn-it-on">Turn it on</h2>
                <pre><code>App::new()
    .database(sqlite_url(&amp;path))                         // where the copy lives
    .backend(Backend::new("https://notes.example.com").offline())</code></pre>
                <p>
                    That is the whole switch, and it changes no command. The design is
                    <a href="../0007-offline-and-sync.html">RFC 0007</a>.
                </p>

                <h2 id="reads">Reads: the last answer</h2>
                <p>
                    Each successful <code>GET</code> keeps its answer, per user, URL and query. A
                    <code>GET</code> that gets no answer returns that copy instead of an
                    <code>offline</code> error; one never read before is still offline.
                    <code>Response::stale_since()</code> says when the copy was made, and
                    <code>.no_offline()</code> keeps a request out of it, for a one-time code or a big
                    report.
                </p>
                <p>
                    The copy lives in the app's database with the user and the URL hashed. It is kept
                    per user, only while signed in, and cleared when the user signs out or someone else
                    signs in. It is capped at 100&nbsp;MiB, oldest first, and an answer over
                    2&nbsp;MiB is not kept.
                </p>
                <p>
                    No answer means offline. While offline, reads answer from the copy at once, and
                    Laravel's health route, <code>/up</code>, is probed every second, backing off to
                    thirty. Any answer means online again, and every live query that read from the
                    backend re-runs.
                </p>

                <h2 id="writes">Writes: the outbox</h2>
                <p>
                    A write marked <code>.queue_offline()</code> that gets no answer waits in the
                    outbox instead of failing:
                </p>
                <pre><code>#[command]
async fn team_notes_store(ctx: Ctx, input: TeamNoteInput) -&gt; Result&lt;TeamNote&gt; {
    let note: TeamNote = ctx.get::&lt;Backend&gt;()
        .post("/api/notes")
        .body(&amp;input)
        .queue_offline()
        .resource()
        .await?;                     // no answer: Err(BackendError::Queued(id)), kind "queued"
    ctx.dispatch(TeamNoteCreated { note: note.clone() }).await?;
    Ok(note)
}</code></pre>
                <ul>
                    <li><strong><code>queued</code> is an error kind, not a value.</strong> The
                        command's types stay what they are, and the code after the write runs only when
                        the write really happened. The page says "Saved, it will be sent when you're
                        back online".</li>
                    <li><strong>Sent in order, once.</strong> When the server is back, writes go one at
                        a time, oldest first, each with the <code>Idempotency-Key</code> it was first
                        tried with. Without <code>.queue_offline()</code> a write fails with
                        <code>offline</code> as before, because a payment should not go later by
                        accident.</li>
                    <li><strong>The server's answer decides.</strong> A 2xx dispatches
                        <code>OutboxSent</code> and the lists re-run. A 422, 403, 404 or another refusal
                        moves the write aside with the server's errors per field, dispatches
                        <code>OutboxFailed</code>, and the next writes go on. A 401 waits until the
                        same user signs in again. No answer, a 5xx or a 429 is tried again with backoff,
                        up to five minutes or the server's <code>Retry-After</code>.</li>
                    <li><strong>Signing out is refused</strong> with kind <code>pending_changes</code>
                        while writes wait or were refused, since they would be lost.
                        <code>auth.signOutDiscarding()</code> signs out anyway, after asking.</li>
                </ul>
                <p>
                    The outbox survives a restart, and the token is never stored in it.
                </p>

                <h2 id="frontend">In the frontend</h2>
                <pre><code>&lt;script&gt;
  import { connection, outbox } from "@elyra/runtime";
&lt;/script&gt;

{#if $connection.state === "offline"}
  &lt;p&gt;Offline since {new Date($connection.since).toLocaleTimeString()}
     &mdash; {$connection.pending} change(s) waiting&lt;/p&gt;
{/if}

{#each $outbox.filter((w) =&gt; w.status === "failed") as w}
  &lt;p&gt;{w.method} {w.path}: {w.error}&lt;/p&gt;
  {#if w.reason === "conflict"}
    &lt;button onclick={() =&gt; outbox.keepMine(w.id)}&gt;Keep mine&lt;/button&gt;
    &lt;button onclick={() =&gt; outbox.takeTheirs(w.id)}&gt;Take theirs&lt;/button&gt;
  {:else}
    &lt;button onclick={() =&gt; outbox.retry(w.id, fixedBody)}&gt;Send again&lt;/button&gt;
    &lt;button onclick={() =&gt; outbox.discard(w.id)}&gt;Discard&lt;/button&gt;
  {/if}
{/each}</code></pre>
                <p>
                    The page can list, retry, resolve and discard writes, and a retry may change the body
                    only. It cannot add a write, change one's method or path, or see the token. A
                    resource generated with <code>--backend</code> writes all of this for him once the
                    app has <code>.offline()</code>: an offline banner, the writes waiting and those
                    refused, and a test that saves with no network and checks the write is sent once.
                </p>

                <h2 id="conflicts">Conflicts</h2>
                <p>
                    By default the last write wins, as in Laravel. On the train Freddy edits a note his
                    colleague is also editing in the office; sent later, his edit would overwrite hers.
                    Where that is wrong, a write carries a precondition:
                </p>
                <pre><code>backend.put(format!("/api/notes/{id}"))
    .body(&amp;input)
    .unless_changed_since(&amp;note.updated_at)   // If-Unmodified-Since
    .queue_offline()</code></pre>
                <p>
                    The server answers 412 when the record changed, and 409 means the same. From the
                    outbox, the write is refused with <code>reason: "conflict"</code>, and Freddy
                    chooses: <em>Keep mine</em> sends it again without the precondition, <em>Take
                    theirs</em> drops it and re-reads the note. On the Laravel side it is one line in the
                    controller:
                </p>
                <pre><code>abort_if($request-&gt;hasHeader('If-Unmodified-Since')
    &amp;&amp; $note-&gt;updated_at-&gt;gt(Carbon::parse($request-&gt;header('If-Unmodified-Since'))), 412,
    'Someone changed this note meanwhile.');</code></pre>

                <h2 id="idempotency">Once, on the Laravel side too</h2>
                <p>
                    A write can reach the server just as the connection drops, so the app never hears
                    the answer and sends it again. The guide's <code>Idempotent</code> middleware, after
                    <code>auth:sanctum</code>, answers a repeated <code>Idempotency-Key</code> from its
                    record for 24 hours per key and user, and remembers only writes that happened, so
                    the note is not created twice. The full middleware is in the
                    <a href="../backend.html">guide</a>.
                </p>""",
    "learned": [
        "<code>.offline()</code> keeps the last answer per user and serves it when there is no answer, without changing a command.",
        "<code>.queue_offline()</code> puts a write in an outbox that sends it once, in order, with its <code>Idempotency-Key</code>; <code>queued</code> is an error kind.",
        "Refused writes wait for the user; signing out with writes waiting is refused.",
        "<code>unless_changed_since</code> turns a stale update into a conflict, resolved with Keep mine or Take theirs.",
    ],
    "next": "Laravel's notifications reach Freddy's desktop.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 14,
    "title": "Laravel's notifications on the desktop",
    "summary": "What $user->notify() sends, heard, shown, clicked, counted, listed and caught up, and rata make:notifications.",
    "lead": (
        "When a colleague mentions Freddy in a note, the Laravel app already sends him a "
        "notification. Now it also reaches his desktop: in the OS when the app is in the "
        "background, as a toast when it is in front, with a count on the icon, and read "
        "once wherever he reads it."
    ),
    "body": r"""                <h2 id="turn-it-on">Turn it on</h2>
                <pre><code>App::new().backend(
    Backend::new("https://notes.example.com")
        .broadcasting(Broadcasting::pusher(key))
        .notifications(),   // App.Models.User.{id}, and /api/notifications
)</code></pre>
                <p>
                    It needs chapter 12's broadcasting, and the app will not start without it. On the
                    Laravel side, Freddy's notification goes through the <code>database</code> and
                    <code>broadcast</code> channels and returns a title, a body and a url, by
                    convention:
                </p>
                <pre><code>class MentionedInNote extends Notification
{
    public function via(object $notifiable): array
    {
        return ['database', 'broadcast'];
    }

    public function toArray(object $notifiable): array
    {
        return ['title' =&gt; 'You were mentioned', 'body' =&gt; 'Ada mentioned you in "Train ideas".', 'url' =&gt; '/team-notes/12'];
    }
}</code></pre>
                <p>
                    <code>MentionedInNote</code> is Freddy's; the guide's example is an
                    <code>InvoicePaid</code> with the same shape.
                </p>

                <h2 id="the-life-of-one">The life of one notification</h2>
                <ul>
                    <li><strong>Heard</strong> on the signed-in user's private channel, Laravel's
                        <code>App.Models.User.{id}</code>, authorized by the backend like any private
                        channel. Signing in as someone else follows theirs.</li>
                    <li><strong>Shown</strong>: with no window of the app in front, the OS shows it,
                        which on macOS wants chapter 8's bundled app; with one in front, it is for the
                        app to toast. <code>.notification_previews(false)</code> shows only "New
                        notification", for a locked screen.</li>
                    <li><strong>Clicked</strong>: the app comes forward, the notification is read, and
                        its url opens when it is a path in the app, <code>/team-notes/12</code>. A full
                        URL is never opened by a click; the app's own listener can choose to open
                        one.</li>
                    <li><strong>Counted</strong>: the unread count, the server's, is on the dock on
                        macOS, the launcher on Linux, or the taskbar button on Windows, and in the
                        tray's tooltip. Signing out clears it.</li>
                    <li><strong>Listed and read</strong>: <code>Notifications::refresh()</code> reads
                        <code>GET /api/notifications</code>, Laravel's paginator with
                        <code>unread</code> beside it. <code>mark_read</code> and
                        <code>mark_all_read</code> count down at once and post to the server, through
                        chapter 13's outbox when offline. The list is read again when a window gets
                        focus, and when the web broadcasts <code>NotificationsRead</code>, so reading it
                        in the browser reads it on the desktop too.</li>
                    <li><strong>Missed</strong> while offline or closed: when the connection is back, or
                        at the start, what came meanwhile is shown, one as itself, more as "4 new
                        notifications" that opens <code>/notifications</code>. A first sign-in on a
                        machine shows nothing, so a new laptop does not greet Freddy with a year of
                        mentions.</li>
                </ul>

                <h2 id="frontend">In the frontend</h2>
                <pre><code>&lt;script&gt;
  import { notifications, onNotification, toast } from "@elyra/runtime";
  onNotification((e) =&gt; e.shown === "toast" &amp;&amp; toast(e.presented.title));
&lt;/script&gt;

&lt;a href="#/notifications"&gt;Notifications {$notifications.unread}&lt;/a&gt;
{#each $notifications.items as n (n.id)}
  &lt;button onclick={() =&gt; notifications.markRead(n.id)}&gt;{n.data.title}&lt;/button&gt;
{/each}</code></pre>
                <p>
                    The <code>notifications</code> store, <code>{ unread, items, hasMore }</code>, is
                    read from the server when first subscribed and then kept up to date. It all goes
                    through the auth capability, and the page may read the list and mark it read,
                    nothing else.
                </p>

                <h2 id="generate-it">rata make:notifications</h2>
                <p>
                    Freddy does not write that page by hand either. <code>rata make:notifications</code>
                    writes the page at <code>#/notifications</code> with Mark all read, More and the
                    offline banner, a nav item with the unread count, and the toasts and click handling,
                    set up once for the whole app. It also prints the Laravel side: the
                    <code>notifications</code> table, a <code>NotificationController</code> with
                    <code>index</code>, <code>read</code> and <code>readAll</code>, its routes under
                    <code>auth:sanctum</code>, and a <code>NotificationsRead</code> event broadcast
                    <code>-&gt;toOthers()</code> when the web reads them.
                </p>
                <p>
                    In Rust, <code>Notifier</code> shows notifications from the container, and the
                    frontend's <code>notify(..)</code> from chapter 8 goes through it, so a test can use
                    <code>Notifier::fake()</code> to record them instead of showing them. The guide is
                    the notifications section of <a href="../backend.html">A Laravel backend</a>, and
                    the design is <a href="../0008-laravel-notifications.html">RFC 0008</a>.
                </p>""",
    "learned": [
        "<code>.notifications()</code>, on top of broadcasting, brings what <code>$user-&gt;notify()</code> sends through <code>database</code> and <code>broadcast</code> to the desktop.",
        "Shown by the OS in the background and toasted in front, a click opens a path in the app and never a full URL by itself.",
        "The unread count is the server's, on the icon and in the tray, and reading in one place reads it everywhere.",
        "What came while offline or closed is caught up, and <code>rata make:notifications</code> writes the page and prints the Laravel side.",
    ],
    "next": "Freddy tests the app and ships it.",
})

# ---------------------------------------------------------------------------
CHAPTERS.append({
    "n": 15,
    "title": "Testing, bundling and the updater",
    "summary": "TestApp and fakes, rata bundle and what it does not do, and updates that only install when their signature verifies.",
    "lead": (
        "Freddy's app does a lot now. Before he gives it to his team he wants three "
        "things: tests that run the real pipeline without a window, an app his "
        "colleagues can double-click, and a way to send them the next version."
    ),
    "body": r"""                <h2 id="tests">Tests through the real pipeline</h2>
                <p>
                    <code>elyra::testing</code> runs the app's commands through the container,
                    providers, middleware and validation, without opening a window:
                </p>
                <pre><code>use elyra::testing::TestApp;

#[tokio::test]
async fn a_note_needs_a_title() {
    let app = TestApp::new(app_for_tests());

    let errors = app.invoke_validation_errors("notes_store", (input_without_title,))
        .await.expect("a validation bag");
    assert!(errors.contains_key("title"));
}</code></pre>
                <p>
                    <code>invoke_ok</code>, <code>invoke_err</code> and <code>invoke_err_kind</code>
                    cover the rest, the last one returning a kind from chapter 3 or 11 such as
                    <code>offline</code> or <code>queued</code>; <code>assert_emitted</code> checks the
                    event bus; and chapter 7's <code>app.live(..)</code> waits for a pushed result.
                </p>
                <p>
                    Fakes are Laravel's <code>Queue::fake()</code>, <code>Storage::fake()</code> and
                    <code>Event::fake()</code>, with <code>Notifier::fake()</code> and
                    <code>Store::fake()</code> beside them. <code>App::swap</code> puts one in place after
                    every provider has registered, so the production wiring stays as it is and the fake
                    still wins:
                </p>
                <pre><code>let app = TestApp::new(
    App::new()
        .provider(StorageProvider::at(data_dir))
        .provider(QueueProvider::new())
        .swap(Storage::fake())
        .swap(Queue::fake())
        .commands(commands![export_all]),
);
app.invoke_ok::&lt;()&gt;("export_all", ()).await;
app.get::&lt;Storage&gt;().assert_exists("exports/train-ideas.md");</code></pre>
                <p>
                    Chapter 4's <code>dyn Exporter</code> fakes the same way, with
                    <code>swap_as</code>. A failed assertion names what did happen and points at
                    Freddy's test line, not into the framework. For the Laravel half,
                    <code>Http::fake(HttpFake)</code> answers HTTP from a table, and the backend guide
                    shows a signed-in app, <code>PusherFake</code> for broadcasting, a write saved with
                    no network, and a notification.
                </p>

                <h2 id="bundle">An app to double-click</h2>
                <p>
                    <code>rata build</code> builds the frontend and a release binary with it embedded.
                    <code>rata bundle</code> then packages it for the platform it runs on, from
                    <code>[bundle]</code> in <code>elyra.toml</code>:
                </p>
                <pre><code>[bundle]
identifier = "com.example.freddynotes"
name = "Freddy Notes"
version = "1.0.0"
deep_link = "freddynotes"        # registers freddynotes:// with the OS</code></pre>
                <table>
                    <thead><tr><th>Host</th><th>What <code>rata bundle</code> makes</th></tr></thead>
                    <tbody>
                        <tr><td>macOS</td><td><code>target/release/bundle/Freddy Notes.app</code>, ad-hoc code-signed so it launches locally</td></tr>
                        <tr><td>Linux</td><td>a <code>.deb</code> and a portable <code>.tar.gz</code>, with the <code>.desktop</code> entry and icon</td></tr>
                        <tr><td>Windows</td><td>a portable folder with the <code>.exe</code>, the icon and a <code>README.txt</code></td></tr>
                    </tbody>
                </table>
                <p>
                    The icon comes from <code>app/public/icon.svg</code>, which <code>rata new</code>
                    scaffolded, and <code>deep_link</code> is what makes chapter 8's links reach the app.
                    A bundled app is also what macOS wants for notifications and autostart.
                </p>
                <div class="callout">
                    <strong>What it does not do.</strong> The <a href="../cli.html">CLI page</a> says it
                    plainly: real Developer ID signing and notarization, MSI and NSIS installers, and
                    AppImage or Flatpak are out of scope, because they need certificates and toolchains
                    that belong to each project. An ad-hoc signed app launches on the Mac that built it;
                    to hand the app to colleagues without Gatekeeper getting in their way, Freddy signs
                    and notarizes it with his own Developer ID as a release step.
                </div>

                <h2 id="updates">The next version</h2>
                <p>
                    With the <code>updater</code> feature, the framework wires up the whole flow: a
                    check, progress events, and a themed update toast in the page with <em>What's
                    new</em>, <em>Install &amp; restart</em> and <em>Later</em>:
                </p>
                <pre><code>App::new()
    .updater(
        UpdaterConfig::new(PUBLIC_KEY_B64, "https://notes.example.com/desktop/latest.json",
                           env!("CARGO_PKG_VERSION"))
            .auto_check(true),
    )
    .allow_frontend(Capability::UpdaterInstall)   // enables the toast's install button</code></pre>
                <p>
                    The security model mirrors Tauri's: each release is an artifact plus an ed25519
                    signature, listed in a JSON manifest, and the app ships the matching public key. An
                    update is only installed after its downloaded bytes verify against that key, so a
                    compromised release server still cannot push a malicious binary. Manifest and
                    artifact must be HTTPS, and the download is capped in size. The startup check is off
                    until <code>auto_check(true)</code>, and installing needs a capability that is
                    opt-in, because it replaces the binary and relaunches.
                </p>
                <p>
                    Two honest notes from the <a href="../updater.html">updater page</a>. Generating the
                    keys and signatures and hosting the manifest is Freddy's release pipeline's job. And
                    replacing a code-signed binary invalidates its signature, so an app distributed
                    through Gatekeeper must be re-signed and notarized as part of each release.
                </p>

                <h2 id="looking-back">Looking back</h2>
                <p>
                    Freddy started with a notes app in a browser. He now has a compiled desktop app with
                    its own database and screens generated from his models; lists that update
                    themselves; a quick-note window behind a shortcut; settings, a keychain and
                    background jobs; AI in the app and an MCP server for agents; and, as the desktop
                    half of his Laravel app, sign-in with the token in the keychain, changes from the
                    web in his open windows, writes that survive a train ride, and Laravel's
                    notifications on his dock. All of it with the building blocks he knew from Laravel,
                    and none of it with a runtime interpreter.
                </p>
                <p>
                    Where to go from here: the <a href="../overview.html">guide</a> has a page for every
                    part of this course and more, the <a href="../roadmap.html">roadmap</a> says what is
                    coming, and the <a href="../changelog.html">changelog</a> says what each release
                    changed. Upgrading means moving the tag in <code>Cargo.toml</code> and the runtime
                    tarball in <code>app/package.json</code> together, then <code>rata ai:update</code>
                    so the coding agent knows the new version too.
                </p>""",
    "learned": [
        "<code>TestApp</code> runs commands through the real pipeline, and <code>App::swap</code> puts fakes in place after the providers.",
        "<code>rata bundle</code> makes an ad-hoc signed <code>.app</code>, a <code>.deb</code> or a portable folder; Developer ID signing and notarization are the project's own release step.",
        "The updater installs only what verifies against the app's ed25519 key, over HTTPS, with install opt-in.",
    ],
    "next": "That is the course. Freddy's notes are on his desktop now, and so, we hope, are yours.",
})
