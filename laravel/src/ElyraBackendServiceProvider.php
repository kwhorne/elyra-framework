<?php

namespace Elyra\Backend;

use Elyra\Backend\Events\AbilitiesChanged;
use Elyra\Backend\Http\Controllers\AbilitiesController;
use Elyra\Backend\Http\Controllers\AssistantController;
use Elyra\Backend\Http\Controllers\NotificationController;
use Elyra\Backend\Http\Middleware\Idempotent;
use Illuminate\Http\Request;
use Illuminate\Routing\Router;
use Illuminate\Support\Facades\Event;
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
    public function register(): void
    {
        $this->mergeConfigFrom(__DIR__.'/../config/elyra.php', 'elyra');
    }

    public function boot(): void
    {
        $this->publishes([__DIR__.'/../config/elyra.php' => config_path('elyra.php')], 'elyra-config');

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

        /*
         * GET /abilities: what the signed-in user may do, as Backend::abilities()
         * reads it (RFC 0010). Call it inside your auth:sanctum group.
         */
        Route::macro('elyraAbilities', function () {
            Route::get('/abilities', AbilitiesController::class);
        });

        /*
         * POST /elyra/assistant/v1/messages: the desktop's assistant reaches
         * Claude through the server, which adds its key and caps what may be
         * asked, as Backend::assistant() expects (RFC 0011). Call it inside
         * your auth:sanctum group; the key is config('elyra.assistant.key').
         */
        Route::macro('elyraAssistant', function () {
            Route::post('/elyra/assistant/v1/messages', AssistantController::class);
        });

        /*
         * spatie/laravel-permission tells when a role or permission is given or
         * taken (with its events enabled, `permission.events_enabled`): the
         * desktop of that user reads its abilities again.
         */
        foreach ([
            'Spatie\\Permission\\Events\\RoleAttachedEvent',
            'Spatie\\Permission\\Events\\RoleDetachedEvent',
            'Spatie\\Permission\\Events\\PermissionAttachedEvent',
            'Spatie\\Permission\\Events\\PermissionDetachedEvent',
        ] as $event) {
            if (class_exists($event)) {
                Event::listen($event, function ($changed) {
                    $model = $changed->model ?? null;
                    if ($model instanceof \Illuminate\Database\Eloquent\Model && method_exists($model, 'getAllPermissions')) {
                        // Telling the desktop mustn't undo the change: it also
                        // reads them again on focus and on reconnect.
                        try {
                            broadcast(new AbilitiesChanged($model));
                        } catch (\Throwable $e) {
                            report($e);
                        }
                    }
                });
            }
        }
    }
}
