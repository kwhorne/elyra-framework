<?php

namespace Elyra\Backend\Events;

use Illuminate\Contracts\Auth\Authenticatable;

/**
 * A user's desktop turned a recording into text through
 * Route::elyraTranscription() (RFC 0032): listen to keep a record of who
 * used how much. It carries neither the audio nor the text, which may be
 * personal data.
 */
class TranscriptionUsed
{
    public function __construct(
        public Authenticatable $user,
        public string $model,
        public int $bytes,
    ) {}
}
