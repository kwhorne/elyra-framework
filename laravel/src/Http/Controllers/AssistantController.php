<?php

namespace Elyra\Backend\Http\Controllers;

use Elyra\Backend\Events\AssistantUsed;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\RateLimiter;

/**
 * POST /elyra/assistant/v1/messages: the desktop's assistant reaches Claude
 * through here (RFC 0011), with the user's Sanctum token. The server's key is
 * added here and never leaves; what may be asked is capped first: the
 * models in config('elyra.assistant.models'), at most `max_tokens`, `per_minute`
 * requests per user, no streaming, and no server tools unless allowed. The
 * tool loop stays on the desktop: its tools are the desktop's commands.
 */
class AssistantController
{
    public function __invoke(Request $request)
    {
        $config = config('elyra.assistant');
        if (empty($config['key'])) {
            return $this->error(503, 'api_error', "The assistant isn't set up on the server.");
        }

        $user = $request->user();
        $allowed = RateLimiter::attempt(
            'elyra-assistant:'.$user->getAuthIdentifier(),
            (int) $config['per_minute'],
            fn () => true,
        );
        if (! $allowed) {
            return $this->error(429, 'rate_limit_error', 'Too many requests to the assistant; try again in a minute.');
        }

        $body = $request->json()->all();
        $model = $body['model'] ?? null;
        if (! is_string($model) || ! in_array($model, $config['models'], true)) {
            return $this->error(422, 'invalid_request_error', 'The model `'.(is_string($model) ? $model : '').'` isn\'t allowed here.');
        }
        if (! empty($body['stream'])) {
            return $this->error(422, 'invalid_request_error', "Streaming isn't proxied.");
        }
        // A server tool (web search, code execution) has a `type`; the
        // desktop's own tools don't.
        foreach ($body['tools'] ?? [] as $tool) {
            if (isset($tool['type']) && $tool['type'] !== 'custom' && ! $config['server_tools']) {
                return $this->error(422, 'invalid_request_error', "Server tools aren't allowed here.");
            }
        }
        $cap = (int) $config['max_tokens'];
        $body['max_tokens'] = min((int) ($body['max_tokens'] ?? $cap), $cap);

        $response = Http::withHeaders([
            'x-api-key' => $config['key'],
            'anthropic-version' => $request->header('anthropic-version', '2026-10-21'),
        ])->timeout(120)->post(rtrim($config['url'], '/').'/v1/messages', $body);

        if ($response->successful()) {
            event(new AssistantUsed(
                $user,
                $model,
                (int) $response->json('usage.input_tokens', 0),
                (int) $response->json('usage.output_tokens', 0),
            ));
        }

        return response($response->body(), $response->status())
            ->header('Content-Type', 'application/json');
    }

    /** An error in the Messages API's shape, which the desktop reads. */
    private function error(int $status, string $type, string $message)
    {
        return response()->json([
            'type' => 'error',
            'error' => ['type' => $type, 'message' => $message],
        ], $status);
    }
}
