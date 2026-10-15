<?php

namespace Elyra\Backend\Http\Controllers;

use Elyra\Backend\Events\NotificationsRead;
use Illuminate\Http\Request;
use Illuminate\Routing\Controller;

/**
 * Laravel's database notifications for the desktop: the list with the
 * unread count, and marking them read, which the user's other devices hear
 * as NotificationsRead.
 */
class NotificationController extends Controller
{
    public function index(Request $request)
    {
        $user = $request->user();

        return $user->notifications()->paginate(20)->toArray()
            + ['unread' => $user->unreadNotifications()->count()];
    }

    public function read(Request $request, string $id)
    {
        $request->user()->notifications()->whereKey($id)->firstOrFail()->markAsRead();
        broadcast(new NotificationsRead($request->user(), [$id]))->toOthers();

        return response()->noContent();
    }

    public function readAll(Request $request)
    {
        $request->user()->unreadNotifications->markAsRead();
        broadcast(new NotificationsRead($request->user()))->toOthers();

        return response()->noContent();
    }
}
