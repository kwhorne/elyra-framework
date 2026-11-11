<?php

namespace Elyra\Backend\Http\Controllers;

use Elyra\Backend\Events\TranscriptionUsed;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\RateLimiter;

/**
 * POST /elyra/transcription/v1/audio/transcriptions: the desktop's
 * recordings (RFC 0032) become text through here, with the user's Sanctum
 * token. The server's OpenAI key is added here and never leaves; what may
 * be sent is capped first: the models in config('elyra.transcription.models'),
 * files up to `max_kb`, and `per_minute` requests per user. The request is
 * OpenAI's own (multipart: `file`, `model`, `language`), passed on as it
 * came, and so is the answer.
 */
class TranscriptionController
{
    public function __invoke(Request $request)
    {
        $config = config('elyra.transcription');
        if (empty($config['key'])) {
            return $this->error(503, 'server_error', "Transcription isn't set up on the server.");
        }

        $user = $request->user();
        $allowed = RateLimiter::attempt(
            'elyra-transcription:'.$user->getAuthIdentifier(),
            (int) $config['per_minute'],
            fn () => true,
        );
        if (! $allowed) {
            return $this->error(429, 'rate_limit_exceeded', 'Too many transcriptions; try again in a minute.');
        }

        $model = $request->input('model');
        if (! is_string($model) || ! in_array($model, $config['models'], true)) {
            return $this->error(422, 'invalid_request_error', 'The model `'.(is_string($model) ? $model : '').'` isn\'t allowed here.');
        }
        $file = $request->file('file');
        if (! $file || ! $file->isValid()) {
            return $this->error(422, 'invalid_request_error', 'A recording is needed, as `file`.');
        }
        $bytes = (int) $file->getSize();
        if ($bytes > (int) $config['max_kb'] * 1024) {
            return $this->error(413, 'invalid_request_error', 'The recording is too large to transcribe here.');
        }

        $fields = ['model' => $model];
        $language = $request->input('language');
        if (is_string($language) && $language !== '') {
            $fields['language'] = $language;
        }

        $response = Http::withToken($config['key'])
            ->timeout(120)
            ->attach('file', $file->get(), $file->getClientOriginalName() ?: 'recording.wav')
            ->post(rtrim($config['url'], '/').'/v1/audio/transcriptions', $fields);

        if ($response->successful()) {
            event(new TranscriptionUsed($user, $model, $bytes));
        }

        return response($response->body(), $response->status())
            ->header('Content-Type', 'application/json');
    }

    /** An error in OpenAI's shape, which the desktop reads. */
    private function error(int $status, string $type, string $message)
    {
        return response()->json([
            'error' => ['type' => $type, 'message' => $message],
        ], $status);
    }
}
