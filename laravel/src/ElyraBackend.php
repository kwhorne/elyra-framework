<?php

namespace Elyra\Backend;

/**
 * The package itself. A path repository installed with `@dev` has no other
 * way to say which release it is (RFC 0057): `php artisan elyra:contract`
 * reports it, `php artisan about` shows it, and `rata check --backend`
 * holds it against the desktop app's.
 */
final class ElyraBackend
{
    /** The Elyra release this package is from, bumped with the rest at a release. */
    public const VERSION = '0.57.0';
}
