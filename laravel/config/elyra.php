<?php

return [
    /*
     * Without spatie/laravel-permission, GET /abilities answers those of
     * these the Gate allows the user (Gate::define, or a policy's ability).
     */
    'abilities' => [
        // 'customers.view', 'customers.create', 'customers.update', 'customers.delete',
    ],

    /*
     * Route::elyraBroadcasting(): where the desktop connects (RFC 0021). Left
     * empty, it's the default broadcast connection's (REVERB_HOST, PUSHER_HOST
     * or the Pusher cluster); set these when the desktop reaches Reverb at
     * another address than Laravel publishes to, as behind a proxy.
     */
    'broadcasting' => [
        'host' => env('ELYRA_BROADCASTING_HOST'),
        'port' => env('ELYRA_BROADCASTING_PORT'),
        // wss, or ws (which the desktop allows only to loopback).
        'scheme' => env('ELYRA_BROADCASTING_SCHEME'),
    ],

    /*
     * Route::elyraUpdates(): the desktop's releases (RFC 0024), put here by
     * `php artisan elyra:release`, each file beside a releases.json. A
     * private disk: they're downloaded through the route, with the user's
     * token.
     */
    'updates' => [
        'disk' => env('ELYRA_UPDATES_DISK', 'local'),
        'path' => env('ELYRA_UPDATES_PATH', 'desktop-releases'),
    ],

    /*
     * Route::elyraAssistant(): the desktop's assistant reaches Claude through
     * the server, with this key, and only within these limits (RFC 0011).
     */
    'assistant' => [
        'key' => env('ANTHROPIC_API_KEY'),
        'url' => env('ELYRA_ASSISTANT_UPSTREAM', 'https://api.anthropic.com'),
        // The models the desktop may ask for.
        'models' => ['claude-sonnet-5-5'],
        // The most a request may generate; a larger ask is lowered to this.
        'max_tokens' => (int) env('ELYRA_ASSISTANT_MAX_TOKENS', 4096),
        // Requests per user per minute.
        'per_minute' => (int) env('ELYRA_ASSISTANT_PER_MINUTE', 30),
        // Claude's own tools (web search, code execution): off.
        'server_tools' => false,
        // The desktop assistant's code mode (RFC 0027): Claude's code calls
        // the app's tools from Anthropic's code execution sandbox. The
        // results of those calls go to the sandbox, kept up to 30 days and
        // not covered by zero data retention: off unless the server says so.
        'code_execution' => (bool) env('ELYRA_ASSISTANT_CODE_EXECUTION', false),
        // The answer as it's written (RFC 0018): passed through as Claude
        // streams it. Behind nginx, `X-Accel-Buffering: no` keeps it from
        // being held back; a server that buffers anyway can turn it off.
        'stream' => (bool) env('ELYRA_ASSISTANT_STREAM', true),
    ],
];
