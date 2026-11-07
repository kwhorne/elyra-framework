<?php

namespace Elyra\Backend;

use Illuminate\Contracts\Filesystem\Filesystem;
use Illuminate\Support\Facades\Storage;

/**
 * The desktop app's releases (RFC 0024), on a Laravel disk: each file
 * beside a releases.json that says what it is. `php artisan elyra:release`
 * adds one; Route::elyraUpdates() serves them.
 */
class Releases
{
    /** `{os}-{arch}`, as the desktop's Updater::current_target() says it. */
    public const PLATFORM = '/^(macos|windows|linux)-(aarch64|x86_64)$/';

    /** A semver version: 1.5.0, 2.0.0-beta.1. */
    public const VERSION = '/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$/';

    public function disk(): Filesystem
    {
        return Storage::disk(config('elyra.updates.disk', 'local'));
    }

    public function path(string $file = ''): string
    {
        $dir = trim(config('elyra.updates.path', 'desktop-releases'), '/');

        return $file === '' ? $dir : "{$dir}/{$file}";
    }

    /** @return list<array{version: string, platform: string, file: string, size: int, sha256: string, notes?: string, released_at: string, signature?: string}> */
    public function all(): array
    {
        $manifest = $this->path('releases.json');
        if (! $this->disk()->exists($manifest)) {
            return [];
        }

        return json_decode($this->disk()->get($manifest), true) ?: [];
    }

    /** The newest release for `$platform` newer than `$version`, or null. */
    public function newerThan(string $platform, string $version): ?array
    {
        $newest = null;
        foreach ($this->all() as $release) {
            if ($release['platform'] !== $platform || ! version_compare($release['version'], $version, '>')) {
                continue;
            }
            if ($newest === null || version_compare($release['version'], $newest['version'], '>')) {
                $newest = $release;
            }
        }

        return $newest;
    }

    public function file(string $file): ?array
    {
        foreach ($this->all() as $release) {
            if ($release['file'] === $file) {
                return $release;
            }
        }

        return null;
    }

    /**
     * Add `$source` as `$version` for `$platform`, replacing a release of
     * the same version and platform. Returns what was recorded.
     */
    public function add(string $source, string $version, string $platform, ?string $notes = null, ?string $signature = null): array
    {
        $file = basename($source);
        $others = array_values(array_filter(
            $this->all(),
            fn (array $r) => ! ($r['version'] === $version && $r['platform'] === $platform),
        ));
        foreach ($others as $other) {
            if ($other['file'] === $file) {
                throw new \InvalidArgumentException(
                    "{$file} is already {$other['platform']} {$other['version']}: give each release's file its own name"
                );
            }
        }

        $stream = fopen($source, 'rb');
        try {
            $this->disk()->writeStream($this->path($file), $stream);
        } finally {
            if (is_resource($stream)) {
                fclose($stream);
            }
        }

        $release = array_filter([
            'version' => $version,
            'platform' => $platform,
            'file' => $file,
            'size' => filesize($source),
            'sha256' => hash_file('sha256', $source),
            'notes' => $notes,
            'released_at' => now()->toDateString(),
            'signature' => $signature,
        ], fn ($v) => $v !== null && $v !== '');

        $others[] = $release;
        $this->disk()->put($this->path('releases.json'), json_encode($others, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES));

        return $release;
    }
}
