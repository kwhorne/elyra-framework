<?php

namespace Elyra\Backend\Events;

use Illuminate\Contracts\Auth\Authenticatable;

/**
 * A user's assistant asked Claude through Route::elyraAssistant() (RFC 0011):
 * listen to keep a record of who used how much. It carries no prompt or
 * answer, which may be personal data.
 */
class AssistantUsed
{
    public function __construct(
        public Authenticatable $user,
        public string $model,
        public int $inputTokens,
        public int $outputTokens,
    ) {}
}
