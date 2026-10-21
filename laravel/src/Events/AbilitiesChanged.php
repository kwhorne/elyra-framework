<?php

namespace Elyra\Backend\Events;

use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Database\Eloquent\Model;

/**
 * What the user may do changed (a role or permission given or taken): the
 * desktop reads its abilities again (RFC 0010). Sent on the user's own
 * channel (App.Models.User.{id}). The package dispatches it for
 * spatie/laravel-permission's events; dispatch it yourself for anything else.
 */
class AbilitiesChanged implements ShouldBroadcastNow
{
    use InteractsWithSockets;

    public function __construct(public Model $user) {}

    public function broadcastOn(): array
    {
        return [new PrivateChannel(str_replace('\\', '.', $this->user::class).'.'.$this->user->getKey())];
    }

    public function broadcastAs(): string
    {
        return 'AbilitiesChanged';
    }

    public function broadcastWith(): array
    {
        return [];
    }
}
