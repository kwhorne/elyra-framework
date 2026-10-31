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
    ],
];
