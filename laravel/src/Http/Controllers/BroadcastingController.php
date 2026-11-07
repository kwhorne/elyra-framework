<?php

namespace Elyra\Backend\Http\Controllers;

/**
 * GET /desktop/broadcasting: where the desktop connects to hear Laravel's
 * broadcasts, as Backend::broadcasting_from() reads it (RFC 0021):
 * { key, host?, port?, scheme? } from the default broadcast connection,
 * Reverb or Pusher. What config('elyra.broadcasting') gives wins, for when
 * the address the desktop reaches differs from the one Laravel publishes to.
 * Without a Reverb or Pusher connection it's a 404, and the desktop runs
 * without broadcasting.
 */
class BroadcastingController
{
    public function __invoke()
    {
        $name = config('broadcasting.default');
        $connection = config("broadcasting.connections.{$name}", []);
        $driver = $connection['driver'] ?? null;

        if (! in_array($driver, ['reverb', 'pusher'], true) || empty($connection['key'])) {
            return response()->json([
                'message' => "Broadcasting isn't set up for the desktop: the default connection is "
                    .($driver ?? 'none').', not Reverb or Pusher with a key.',
            ], 404);
        }

        $options = $connection['options'] ?? [];
        $public = array_filter(config('elyra.broadcasting', []), fn ($v) => $v !== null && $v !== '');

        $host = $public['host'] ?? ($options['host'] ?? null);
        // Pusher's own config names its REST host; the socket is ws-<cluster>.
        if ($driver === 'pusher' && is_string($host) && preg_match('/^api-(.+)\.pusher\.com$/', $host, $m)) {
            $host = "ws-{$m[1]}.pusher.com";
        } elseif ($driver === 'pusher' && ! $host && ! empty($options['cluster'])) {
            $host = "ws-{$options['cluster']}.pusher.com";
        }

        $scheme = $public['scheme'] ?? match ($options['scheme'] ?? null) {
            'https' => 'wss',
            'http' => 'ws',
            default => null,
        };

        return array_filter([
            'key' => $connection['key'],
            'host' => $host,
            'port' => isset($public['port']) || isset($options['port'])
                ? (int) ($public['port'] ?? $options['port'])
                : null,
            'scheme' => $scheme,
        ], fn ($v) => $v !== null && $v !== '');
    }
}
