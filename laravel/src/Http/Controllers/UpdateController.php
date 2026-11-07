<?php

namespace Elyra\Backend\Http\Controllers;

use Elyra\Backend\Releases;
use Illuminate\Http\Request;

/**
 * GET /desktop/update?platform=&version= and GET /desktop/download/{file}:
 * the desktop's updates from this server, as UpdaterConfig::from_backend()
 * reads them (RFC 0024). A 204 when it's up to date; else the newest
 * release for its platform, whose file is downloaded from this server, with
 * the same token.
 */
class UpdateController
{
    public function check(Request $request, Releases $releases)
    {
        $request->validate([
            'platform' => ['required', 'string', 'regex:'.Releases::PLATFORM],
            'version' => ['required', 'string', 'regex:'.Releases::VERSION],
        ]);

        $release = $releases->newerThan($request->query('platform'), $request->query('version'));
        if ($release === null) {
            return response()->noContent();
        }

        return array_filter([
            'version' => $release['version'],
            'notes' => $release['notes'] ?? null,
            'released_at' => $release['released_at'] ?? null,
            // A path: the desktop resolves it against the server it asked,
            // whatever APP_URL says, and downloads only from that origin.
            'url' => route('elyra.desktop.download', ['file' => $release['file']], false),
            'size' => $release['size'],
            'sha256' => $release['sha256'],
            'signature' => $release['signature'] ?? null,
        ], fn ($v) => $v !== null);
    }

    public function download(string $file, Releases $releases)
    {
        $release = $releases->file($file);
        abort_if($release === null, 404);

        return $releases->disk()->download($releases->path($release['file']), $release['file'], [
            'Content-Type' => 'application/octet-stream',
        ]);
    }
}
