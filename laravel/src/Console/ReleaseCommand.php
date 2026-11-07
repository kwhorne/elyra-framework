<?php

namespace Elyra\Backend\Console;

use Elyra\Backend\Releases;
use Illuminate\Console\Command;

/**
 * php artisan elyra:release Felagi-1.5.0-macos-aarch64.zip 1.5.0 macos-aarch64
 *
 * Puts a desktop release on the updates disk, with its size and SHA-256,
 * for Route::elyraUpdates() to offer (RFC 0024).
 */
class ReleaseCommand extends Command
{
    protected $signature = 'elyra:release
        {file : The release: a zip of the signed .app on macOS, as the desktop installs it}
        {release : Its version, semver (1.5.0)}
        {platform : macos-aarch64, macos-x86_64, windows-x86_64, linux-x86_64, …}
        {--notes= : What changed, shown in the update toast}
        {--notes-file= : The same, read from a file}
        {--signature= : Base64 ed25519 signature of the file, for an app with a public_key}';

    protected $description = 'Offer a new version of the desktop app to its users (Route::elyraUpdates)';

    public function handle(Releases $releases): int
    {
        $file = $this->argument('file');
        $version = $this->argument('release');
        $platform = $this->argument('platform');

        if (! is_file($file)) {
            $this->error("No file at {$file}.");

            return self::FAILURE;
        }
        if (! preg_match(Releases::VERSION, $version)) {
            $this->error("{$version} isn't a semver version (1.5.0).");

            return self::FAILURE;
        }
        if (! preg_match(Releases::PLATFORM, $platform)) {
            $this->error("{$platform} isn't a platform the desktop asks for: {os}-{arch}, as macos-aarch64 or windows-x86_64.");

            return self::FAILURE;
        }

        $notes = $this->option('notes');
        if ($path = $this->option('notes-file')) {
            if (! is_file($path)) {
                $this->error("No notes file at {$path}.");

                return self::FAILURE;
            }
            $notes = trim(file_get_contents($path));
        }

        try {
            $release = $releases->add($file, $version, $platform, $notes, $this->option('signature'));
        } catch (\InvalidArgumentException $e) {
            $this->error($e->getMessage());

            return self::FAILURE;
        }

        $this->info("{$platform} {$version} is offered: {$release['file']}, {$release['size']} bytes, sha256 {$release['sha256']}.");

        return self::SUCCESS;
    }
}
