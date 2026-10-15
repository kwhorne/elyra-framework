<?php

namespace Elyra\Backend;

use Elyra\Backend\Http\Controllers\NotificationController;
use Elyra\Backend\Http\Middleware\Idempotent;
use Illuminate\Http\Request;
use Illuminate\Routing\Router;
use Illuminate\Support\Facades\Hash;
use Illuminate\Support\Facades\Route;
use Illuminate\Support\ServiceProvider;
use Illuminate\Validation\ValidationException;

/**
 * Opt-in routes for an Elyra desktop app, as docs/backend.md gives them.
 * Nothing is registered until routes/api.php calls a macro.
 */
class ElyraBackendServiceProvider extends ServiceProvider
{
    public function boot(): void
    {
        $this->app->make(Router::class)->aliasMiddleware('elyra.idempotent', Idempotent::class);

        /*
         * POST /sanctum/token (email, password, device_name) answers the plain-text
         * token, DELETE /sanctum/token revokes it and GET /user is the signed-in user:
         * Elyra's Backend::new(url) defaults, under routes/api.php's /api prefix.
         * The user model needs Sanctum's HasApiTokens.
         */
        Route::macro('elyraAuth', function (?string $userModel = null) {
            $userModel ??= config('auth.providers.users.model', 'App\\Models\\User');

            Route::post('/sanctum/token', function (Request $request) use ($userModel) {
                $request->validate([
                    'email' => 'required|email',
                    'password' => 'required',
                    'device_name' => 'required',
                ]);
                $user = $userModel::where('email', $request->email)->first();
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
            });
        });

        /*
         * GET /notifications (the paginator with `unread`), POST /notifications/read-all
         * and POST /notifications/{id}/read, as Backend::notifications() reads them.
         * Call it inside your auth:sanctum group.
         */
        Route::macro('elyraNotifications', function () {
            Route::get('/notifications', [NotificationController::class, 'index']);
            Route::post('/notifications/read-all', [NotificationController::class, 'readAll']);
            Route::post('/notifications/{id}/read', [NotificationController::class, 'read']);
        });
    }
}
