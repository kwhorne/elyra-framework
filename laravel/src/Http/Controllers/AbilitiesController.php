<?php

namespace Elyra\Backend\Http\Controllers;

use Illuminate\Http\Request;
use Illuminate\Support\Facades\Gate;

/**
 * GET /abilities: what the signed-in user may do, as the desktop's
 * Backend::abilities() reads it (RFC 0010). With spatie/laravel-permission,
 * the user's permissions (their roles' included); otherwise every ability
 * in config('elyra.abilities') the Gate allows them.
 */
class AbilitiesController
{
    public function __invoke(Request $request)
    {
        $user = $request->user();

        if (method_exists($user, 'getAllPermissions')) {
            return $user->getAllPermissions()->pluck('name')->unique()->values();
        }

        return collect(config('elyra.abilities', []))
            ->filter(fn (string $ability) => Gate::forUser($user)->allows($ability))
            ->values();
    }
}
