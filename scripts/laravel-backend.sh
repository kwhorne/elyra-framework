#!/usr/bin/env bash
# A real Laravel API for RFC 0005's end-to-end test: a fresh Laravel app with
# Sanctum, the three routes docs/backend.md asks for, a `customers`
# apiResource (validated, paginated, searchable), and a user to sign in as —
# ada@example.com / secret.
#
#   scripts/laravel-backend.sh <dir>          # build it
#   (cd <dir> && php artisan serve --port 8765)
#   ELYRA_LARAVEL_URL=http://127.0.0.1:8765 cargo test -p elyra --features backend --test laravel_backend
#
# Needs PHP 8.3+ and Composer. Runs locally too.
set -euo pipefail

dir="${1:?usage: scripts/laravel-backend.sh <dir>}"
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

# The routes from docs/backend.md, and a resource.
cat >routes/api.php <<'PHP'
<?php

use App\Http\Controllers\CustomerController;
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

Route::middleware('auth:sanctum')->group(function () {
    Route::delete('/sanctum/token', fn (Request $request) => $request->user()->currentAccessToken()->delete());
    Route::get('/user', fn (Request $request) => $request->user());
    Route::apiResource('customers', CustomerController::class);
});
PHP

php artisan make:model Customer --migration --quiet
migration=$(ls database/migrations/*_create_customers_table.php)
python3 - "$migration" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
s = p.read_text()
s = s.replace("$table->id();", "$table->id();\n            $table->string('name');\n            $table->string('email')->unique();", 1)
p.write_text(s)
PY
cat >app/Models/Customer.php <<'PHP'
<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class Customer extends Model
{
    protected $fillable = ['name', 'email'];
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
        return Customer::create($request->validate([
            'name' => 'required|string|max:255',
            'email' => 'required|email|unique:customers',
        ]));
    }

    public function show(Customer $customer)
    {
        return $customer;
    }

    public function update(Request $request, Customer $customer)
    {
        $customer->update($request->validate([
            'name' => 'sometimes|required|string|max:255',
            'email' => ['sometimes', 'required', 'email', Rule::unique('customers')->ignore($customer)],
        ]));
        return $customer;
    }

    public function destroy(Customer $customer)
    {
        $customer->delete();
        return response()->noContent();
    }
}
PHP

php artisan migrate --force --quiet
php artisan tinker --execute='App\Models\User::factory()->create(["name" => "Ada", "email" => "ada@example.com", "password" => "secret"]);' >/dev/null
echo "laravel-backend: ready in $dir (ada@example.com / secret)"
