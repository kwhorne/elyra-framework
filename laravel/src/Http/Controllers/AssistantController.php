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
 * requests per user, and no server tools unless allowed. A request with
 * `stream: true` is passed through as Claude writes it (RFC 0018), unless
 * `stream` is off. The tool loop stays on the desktop: its tools are the
 * desktop's commands.
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
        $stream = ! empty($body['stream']);
        if ($stream && ! ($config['stream'] ?? true)) {
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

        $upstream = Http::withHeaders([
            'x-api-key' => $config['key'],
            'anthropic-version' => $request->header('anthropic-version', '2026-10-21'),
        ])->timeout(120);
        $url = rtrim($config['url'], '/').'/v1/messages';

        if ($stream) {
            return $this->stream($upstream, $url, $body, $user, $model);
        }

        $response = $upstream->post($url, $body);

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

    /**
     * Pass Claude's stream through as it comes: the events unchanged, and the
     * usage read off them on the way for AssistantUsed. An error before the
     * stream starts is answered as it came.
     */
    private function stream($upstream, string $url, array $body, $user, string $model)
    {
        $response = $upstream->withOptions(['stream' => true])->post($url, $body);
        // An error, or an upstream that answered whole: as it came. A
        // streamed body reads once, so it's read once.
        if (! $response->successful() || ! str_starts_with((string) $response->header('Content-Type'), 'text/event-stream')) {
            $whole = $response->body();
            if ($response->successful()) {
                $usage = json_decode($whole, true)['usage'] ?? [];
                event(new AssistantUsed(
                    $user,
                    $model,
                    (int) ($usage['input_tokens'] ?? 0),
                    (int) ($usage['output_tokens'] ?? 0),
                ));
            }

            return response($whole, $response->status())
                ->header('Content-Type', 'application/json');
        }

        return response()->stream(function () use ($response, $user, $model) {
            $source = $response->toPsrResponse()->getBody();
            $line = '';
            $input = 0;
            $output = 0;
            while (! $source->eof()) {
                $chunk = $source->read(1024);
                if ($chunk === '') {
                    continue;
                }
                echo $chunk;
                if (ob_get_level() > 0) {
                    ob_flush();
                }
                flush();
                // The usage, from the events as they pass.
                $line .= $chunk;
                while (($at = strpos($line, "\n")) !== false) {
                    $data = trim(substr($line, 0, $at));
                    $line = substr($line, $at + 1);
                    if (! str_starts_with($data, 'data:')) {
                        continue;
                    }
                    $event = json_decode(trim(substr($data, 5)), true);
                    if (($event['type'] ?? null) === 'message_start') {
                        $input = (int) ($event['message']['usage']['input_tokens'] ?? 0);
                    } elseif (($event['type'] ?? null) === 'message_delta') {
                        $output = (int) ($event['usage']['output_tokens'] ?? $output);
                    }
                }
            }
            event(new AssistantUsed($user, $model, $input, $output));
        }, 200, [
            'Content-Type' => 'text/event-stream',
            'Cache-Control' => 'no-cache',
            'X-Accel-Buffering' => 'no',
        ]);
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
