#!/usr/bin/env bash
# A real Laravel API for RFC 0005's, 0006's and 0007's end-to-end tests: a
# fresh Laravel app with Sanctum, the three routes docs/backend.md asks for, a
# `customers` apiResource (validated, paginated, searchable) that broadcasts
# its changes on `private-customers` (Reverb installed; point it at Askr's
# `--pusher` instead with REVERB_HOST/PORT), answers a repeated
# Idempotency-Key from its record and refuses a stale update with a 412,
# notifies the user of each customer created (RFC 0008: the `database` and
# `broadcast` channels, the routes `rata make:notifications` prints), takes a
# customer's contract as an upload and hands it back as a download (RFC 0009,
# what `make:resource --backend` prints for a `file` field), and a user to
# sign in as — ada@example.com / secret, who may do everything with customers,
# and bob@example.com / secret, who may only see them (RFC 0010: spatie's
# permissions, read through this repository's elyracode/elyra-backend), and
# a presence channel for each customer, `presence-customers.{id}`, that lets
# in whoever may view it (RFC 0015).
#
#   scripts/laravel-backend.sh <dir>          # build it
#   (cd <dir> && php artisan serve --port 8765 & php artisan reverb:start --port 8080 &)
#   ELYRA_LARAVEL_URL=http://127.0.0.1:8765 cargo test -p elyra --features backend --test laravel_backend
#   ELYRA_LARAVEL_URL=http://127.0.0.1:8765 ELYRA_PUSHER="$(grep ^REVERB_APP_KEY= <dir>/.env | cut -d= -f2)@127.0.0.1:8080" \
#     cargo test -p elyra --all-features --test laravel_broadcasting
#   ELYRA_LARAVEL_URL=http://127.0.0.1:8765 cargo test -p elyra --features backend,database --test laravel_offline
#   ELYRA_LARAVEL_URL=http://127.0.0.1:8765 ELYRA_PUSHER="$(grep ^REVERB_APP_KEY= <dir>/.env | cut -d= -f2)@127.0.0.1:8080" \
#     cargo test -p elyra --features backend --test laravel_notifications
#   ELYRA_LARAVEL_URL=http://127.0.0.1:8765 cargo test -p elyra --features backend,database --test laravel_files
#   ELYRA_LARAVEL_URL=http://127.0.0.1:8765 ELYRA_PUSHER="$(grep ^REVERB_APP_KEY= <dir>/.env | cut -d= -f2)@127.0.0.1:8080" \
#     cargo test -p elyra --features backend,database --test laravel_abilities
#   ELYRA_LARAVEL_URL=http://127.0.0.1:8765 ELYRA_ASSISTANT_UPSTREAM=127.0.0.1:8766 \
#     cargo test -p elyra --features ai,backend --test laravel_assistant
#   ELYRA_LARAVEL_URL=http://127.0.0.1:8765 ELYRA_PUSHER="$(grep ^REVERB_APP_KEY= <dir>/.env | cut -d= -f2)@127.0.0.1:8080" \
#     cargo test -p elyra --features backend --test laravel_presence
#
# Needs PHP 8.3+ and Composer. Runs locally too.
set -euo pipefail

dir="${1:?usage: scripts/laravel-backend.sh <dir>}"
repo="$(cd "$(dirname "$0")/.." && pwd)"
composer create-project laravel/laravel "$dir" --prefer-dist --no-interaction --quiet
cd "$dir"
php artisan install:api --without-migration-prompt --no-interaction >/dev/null

# The user model issues tokens.
python3 - <<'PY'
import pathlib
p = pathlib.Path("app/Models/User.php")
s = p.read_text()
s = s.replace("use Illuminate\\Notifications\\Notifiable;",
              "use Illuminate\\Notifications\\Notifiable;\nuse Laravel\\Sanctum\\HasApiTokens;", 1)
s = s.replace("use HasFactory, Notifiable;", "use HasApiTokens, HasFactory, Notifiable;", 1)
assert "HasApiTokens, HasFactory" in s, "the User model changed shape"
p.write_text(s)
PY

# RFC 0010: spatie/laravel-permission, and this repository's
# elyracode/elyra-backend for Route::elyraAbilities() and AbilitiesChanged.
composer config repositories.elyra "{\"type\": \"path\", \"url\": \"$repo/laravel\", \"options\": {\"symlink\": false}}"
composer require "elyracode/elyra-backend:@dev" spatie/laravel-permission --no-interaction --quiet
php artisan vendor:publish --provider="Spatie\Permission\PermissionServiceProvider" --quiet
python3 - <<'PY'
import pathlib, re
p = pathlib.Path("app/Models/User.php")
s = p.read_text()
s = s.replace("use Laravel\\Sanctum\\HasApiTokens;", "use Laravel\\Sanctum\\HasApiTokens;\nuse Spatie\\Permission\\Traits\\HasRoles;", 1)
s = s.replace("use HasApiTokens, HasFactory, Notifiable;", "use HasApiTokens, HasFactory, HasRoles, Notifiable;", 1)
assert "HasRoles, Notifiable" in s, "the User model changed shape"
p.write_text(s)
# spatie tells when a permission is given or taken: the desktop hears it.
p = pathlib.Path("config/permission.php")
s = p.read_text()
s, n = re.subn(r"'events_enabled'\s*=>\s*false", "'events_enabled' => true", s)
assert n == 1, "config/permission.php has no events_enabled"
p.write_text(s)
PY


# The routes from docs/backend.md, and a resource.
cat >routes/api.php <<'PHP'
<?php

use App\Http\Controllers\CustomerController;
use App\Http\Controllers\NotificationController;
use App\Http\Middleware\Idempotent;
use App\Models\User;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Facades\Route;
use Illuminate\Validation\ValidationException;

Route::post('/sanctum/token', function (Request $request) {
    $request->validate([
        'email' => 'required|email',
        'password' => 'required',
        'device_name' => 'required',
    ]);
    $user = User::where('email', $request->email)->first();
    if (! $user || ! Hash::check($request->password, $user->password)) {
        throw ValidationException::withMessages([
            'email' => ['The provided credentials are incorrect.'],
        ]);
    }
    return $user->createToken($request->device_name)->plainTextToken;
});

// Idempotent after auth:sanctum, so it knows whose key it is (RFC 0007).
Route::middleware(['auth:sanctum', Idempotent::class])->group(function () {
    Route::delete('/sanctum/token', fn (Request $request) => $request->user()->currentAccessToken()->delete());
    Route::get('/user', fn (Request $request) => $request->user());
    Route::apiResource('customers', CustomerController::class);
    // RFC 0010: what the signed-in user may do, from the package.
    Route::elyraAbilities();
    // RFC 0011: the desktop's assistant reaches Claude through here.
    Route::elyraAssistant();
    // For the tests: give a user a permission, or take it, as an admin
    // would on the web.
    Route::post('/test/permissions', function (Request $request) {
        $user = User::where('email', $request->input('email'))->firstOrFail();
        $request->boolean('revoke')
            ? $user->revokePermissionTo($request->input('permission'))
            : $user->givePermissionTo($request->input('permission'));
        return $user->getAllPermissions()->pluck('name');
    });
    // RFC 0009: a customer's contract, downloaded.
    Route::get('/customers/{customer}/contract', fn (\App\Models\Customer $customer) =>
        \Illuminate\Support\Facades\Storage::download($customer->contract_path, $customer->contract));
    // Laravel's notifications (RFC 0008), as `rata make:notifications` prints them.
    Route::get('/notifications', [NotificationController::class, 'index']);
    Route::post('/notifications/read-all', [NotificationController::class, 'readAll']);
    Route::post('/notifications/{id}/read', [NotificationController::class, 'read']);
});
PHP

# Working offline (RFC 0007): a write sent again with its Idempotency-Key is
# answered from the record — the middleware docs/backend.md gives.
mkdir -p app/Http/Middleware
cat >app/Http/Middleware/Idempotent.php <<'PHP'
<?php

namespace App\Http\Middleware;

use Closure;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Cache;

class Idempotent
{
    /** A write that happened, answered again for 24 h per key and user. */
    public function handle(Request $request, Closure $next)
    {
        $key = $request->header('Idempotency-Key');
        if (! $key || $request->isMethodSafe()) {
            return $next($request);
        }
        $cacheKey = 'idempotency:'.$request->user()?->id.':'.$key;
        if ($seen = Cache::get($cacheKey)) {
            return response($seen['body'], $seen['status'], ['Content-Type' => 'application/json']);
        }
        $response = $next($request);
        if ($response->isSuccessful()) {
            Cache::put($cacheKey, ['status' => $response->getStatusCode(), 'body' => $response->getContent()], now()->addDay());
        }
        return $response;
    }
}
PHP

php artisan make:model Customer --migration --quiet
migration=$(ls database/migrations/*_create_customers_table.php)
python3 - "$migration" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
s = p.read_text()
s = s.replace("$table->id();", "$table->id();\n            $table->string('name');\n            $table->string('email')->unique();\n            $table->string('contract')->nullable();\n            $table->string('contract_path')->nullable();", 1)
p.write_text(s)
PY
cat >app/Models/Customer.php <<'PHP'
<?php

namespace App\Models;

use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Database\Eloquent\BroadcastsEvents;
use Illuminate\Database\Eloquent\Model;

class Customer extends Model
{
    use BroadcastsEvents;

    protected $fillable = ['name', 'email'];

    /** Where the contract is stored isn't the API's business. */
    protected $hidden = ['contract_path'];

    /** Model broadcasting: every change, on `private-customers`. */
    public function broadcastOn(string $event): array
    {
        return [new PrivateChannel('customers')];
    }
}
PHP

cat >app/Http/Controllers/CustomerController.php <<'PHP'
<?php

namespace App\Http\Controllers;

use App\Models\Customer;
use Illuminate\Http\Request;
use Illuminate\Validation\Rule;

class CustomerController extends Controller
{
    /** What `make:resource --backend` sends: search, sort, direction, per_page. */
    public function index(Request $request)
    {
        $sort = in_array($request->input('sort'), ['name', 'email', 'id']) ? $request->input('sort') : 'id';
        return Customer::query()
            ->when($request->input('search'), fn ($q, $term) => $q->where('name', 'like', "%{$term}%"))
            ->orderBy($sort, $request->input('direction') === 'desc' ? 'desc' : 'asc')
            ->paginate(min($request->integer('per_page', 25), 100));
    }

    public function store(Request $request)
    {
        $customer = new Customer($request->validate([
            'name' => 'required|string|max:255',
            'email' => 'required|email|unique:customers',
        ]));
        $this->contract($request, $customer);
        $customer->save();
        // RFC 0008: the user hears of it on every device, the desktop too.
        $request->user()->notify(new \App\Notifications\CustomerCreated($customer));
        return $customer;
    }

    public function show(Customer $customer)
    {
        return $customer;
    }

    public function update(Request $request, Customer $customer)
    {
        // A write made offline (RFC 0007): only if nobody changed it since.
        if ($since = $request->header('If-Unmodified-Since')) {
            abort_if($customer->updated_at->gt(\Illuminate\Support\Carbon::parse($since)), 412,
                'Someone changed this customer meanwhile.');
        }
        $customer->fill($request->validate([
            'name' => 'sometimes|required|string|max:255',
            'email' => ['sometimes', 'required', 'email', Rule::unique('customers')->ignore($customer)],
        ]));
        $this->contract($request, $customer);
        $customer->save();
        return $customer;
    }

    /** RFC 0009: the contract arrives as an upload beside the fields. */
    private function contract(Request $request, Customer $customer): void
    {
        $request->validate(['contract' => ['nullable', 'file', 'mimes:pdf', 'max:20480']]);
        if ($request->hasFile('contract')) {
            $customer->contract_path = $request->file('contract')->store('contracts');
            $customer->contract = $request->file('contract')->getClientOriginalName();
        }
    }

    public function destroy(Request $request, Customer $customer)
    {
        // RFC 0010: the server decides, whatever the desktop shows.
        abort_unless($request->user()->can('customers.delete'), 403, 'You may not delete customers.');
        $customer->delete();
        return response()->noContent();
    }
}
PHP

# Laravel's notifications (RFC 0008): one for each customer created, kept for
# the list and broadcast on the user's channel; the list and its reads; and
# "read on the web", broadcast — what `rata make:notifications` prints.
mkdir -p app/Notifications app/Events
cat >app/Notifications/CustomerCreated.php <<'PHP'
<?php

namespace App\Notifications;

use App\Models\Customer;
use Illuminate\Notifications\Notification;

class CustomerCreated extends Notification
{
    public function __construct(public Customer $customer) {}

    public function via(object $notifiable): array
    {
        return ['database', 'broadcast'];
    }

    public function toArray(object $notifiable): array
    {
        return [
            'title' => 'New customer',
            'body' => $this->customer->name,
            'url' => "/customers/{$this->customer->id}",
        ];
    }
}
PHP

cat >app/Events/NotificationsRead.php <<'PHP'
<?php

namespace App\Events;

use App\Models\User;
use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;

class NotificationsRead implements ShouldBroadcastNow
{
    use InteractsWithSockets;

    public function __construct(public User $user, public ?array $ids = null) {}

    public function broadcastOn(): array
    {
        return [new PrivateChannel('App.Models.User.'.$this->user->id)];
    }

    public function broadcastAs(): string
    {
        return 'NotificationsRead';
    }

    public function broadcastWith(): array
    {
        return ['ids' => $this->ids];
    }
}
PHP

cat >app/Http/Controllers/NotificationController.php <<'PHP'
<?php

namespace App\Http\Controllers;

use App\Events\NotificationsRead;
use Illuminate\Http\Request;

class NotificationController extends Controller
{
    public function index(Request $request)
    {
        $user = $request->user();
        return $user->notifications()->paginate(20)->toArray()
            + ['unread' => $user->unreadNotifications()->count()];
    }

    public function read(Request $request, string $id)
    {
        $request->user()->notifications()->whereKey($id)->firstOrFail()->markAsRead();
        broadcast(new NotificationsRead($request->user(), [$id]))->toOthers();
        return response()->noContent();
    }

    public function readAll(Request $request)
    {
        $request->user()->unreadNotifications->markAsRead();
        broadcast(new NotificationsRead($request->user()))->toOthers();
        return response()->noContent();
    }
}
PHP
php artisan make:notifications-table --quiet

# RFC 0015: the policy the presence rule asks, from spatie's permissions.
mkdir -p app/Policies
cat >app/Policies/CustomerPolicy.php <<'PHP'
<?php

namespace App\Policies;

use App\Models\Customer;
use App\Models\User;

class CustomerPolicy
{
    public function view(User $user, Customer $customer): bool
    {
        return $user->can('customers.view');
    }
}
PHP

# Broadcasting: Reverb, the channel auth route under `api` with Sanctum (as
# docs/backend.md says), who may hear `customers`, and events sent at once.
php artisan install:broadcasting --reverb --without-node --no-interaction >/dev/null
python3 - <<'PY'
import pathlib, re
p = pathlib.Path("bootstrap/app.php")
s = p.read_text()
s = s.replace("        channels: __DIR__.'/../routes/channels.php',\n", "", 1)
s = s.replace(
    "    ->withMiddleware(",
    "    ->withBroadcasting(\n"
    "        __DIR__.'/../routes/channels.php',\n"
    "        ['prefix' => 'api', 'middleware' => ['api', 'auth:sanctum']],\n"
    "    )\n"
    "    ->withMiddleware(",
    1,
)
assert "withBroadcasting" in s, "bootstrap/app.php changed shape"
p.write_text(s)
p = pathlib.Path("routes/channels.php")
p.write_text(p.read_text() + "\nBroadcast::channel('customers', fn ($user) => true);\n"
             # RFC 0015: who has a customer open, the rule docs/backend.md gives.
             "\nBroadcast::channel('customers.{id}', fn ($user, $id) =>\n"
             "    $user->can('view', App\\Models\\Customer::find($id))\n"
             "        ? ['id' => $user->id, 'name' => $user->name]\n"
             "        : false);\n")
p = pathlib.Path(".env")
p.write_text(re.sub(r"^QUEUE_CONNECTION=.*$", "QUEUE_CONNECTION=sync", p.read_text(), flags=re.M))
# RFC 0011: the server's key, and Anthropic is the test's stand-in on 8766.
p.write_text(p.read_text() + "\nANTHROPIC_API_KEY=sk-ant-server-test\n"
             "ELYRA_ASSISTANT_UPSTREAM=http://127.0.0.1:8766\n"
             "ELYRA_ASSISTANT_PER_MINUTE=10\n")
PY

php artisan migrate --force --quiet
# Seeded before Reverb runs: nobody to tell yet.
BROADCAST_CONNECTION=null php artisan tinker --execute='
foreach (["customers.view", "customers.create", "customers.update", "customers.delete", "customers.export"] as $name) {
    Spatie\Permission\Models\Permission::create(["name" => $name, "guard_name" => "web"]);
}
App\Models\User::factory()->create(["name" => "Ada", "email" => "ada@example.com", "password" => "secret"])
    ->givePermissionTo(["customers.view", "customers.create", "customers.update", "customers.delete"]);
App\Models\User::factory()->create(["name" => "Bob", "email" => "bob@example.com", "password" => "secret"])
    ->givePermissionTo("customers.view");
' >/dev/null
echo "laravel-backend: ready in $dir (ada@example.com / secret, bob@example.com / secret)"
