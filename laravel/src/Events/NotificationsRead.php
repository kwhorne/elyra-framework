<?php

namespace Elyra\Backend\Events;

use Illuminate\Broadcasting\InteractsWithSockets;
use Illuminate\Broadcasting\PrivateChannel;
use Illuminate\Contracts\Broadcasting\ShouldBroadcastNow;
use Illuminate\Database\Eloquent\Model;

/**
 * Notifications were read on another device: `ids`, or all of them when null.
 * Sent on the user's own channel (App.Models.User.{id}), where the desktop
 * listens for their notifications.
 */
class NotificationsRead implements ShouldBroadcastNow
{
    use InteractsWithSockets;

    public function __construct(public Model $user, public ?array $ids = null) {}

    public function broadcastOn(): array
    {
        return [new PrivateChannel(str_replace('\\', '.', $this->user::class).'.'.$this->user->getKey())];
    }

    public function broadcastAs(): string
    {
        return 'NotificationsRead';
    }

    public function broadcastWith(): array
    {
        return ['ids' => $this->ids];
    }
}
