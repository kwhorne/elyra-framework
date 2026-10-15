<?php

namespace Elyra\Backend\Http\Middleware;

use Closure;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Cache;
use Symfony\Component\HttpFoundation\Response;

/**
 * A write sent again with the same Idempotency-Key (the desktop's outbox,
 * after a connection dropped) is answered from the record instead of made
 * twice. Only writes that happened are remembered, for 24 hours, per key
 * and user. Put it after auth:sanctum so it knows whose key it is.
 */
class Idempotent
{
    public function handle(Request $request, Closure $next): Response
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
