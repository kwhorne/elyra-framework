<?php

namespace Elyra\Backend\Concerns;

use Illuminate\Http\Request;
use Illuminate\Support\Carbon;

/**
 * On a model: whether it changed after the version the desktop's write was
 * made from (RFC 0007, RFC 0038). The desktop sends that version as
 * If-Unmodified-Since: the Last-Modified its download had, or the
 * updated_at it read. A write that finds the record changed answers 412,
 * and the desktop shows a conflict instead of overwriting someone else's
 * change:
 *
 *     abort_if($attachment->changedSince($request), 412, 'Someone changed it meanwhile.');
 *
 * Compared to the second, as Last-Modified is. No header, or one that isn't
 * a date, isn't a check: the write goes ahead, as HTTP says.
 */
trait ChecksUnmodifiedSince
{
    public function changedSince(Request $request): bool
    {
        $since = $request->header('If-Unmodified-Since');
        $column = $this->getUpdatedAtColumn();
        if (! $since || ! $column || ! $this->{$column}) {
            return false;
        }
        try {
            $at = Carbon::parse($since);
        } catch (\Throwable) {
            return false;
        }

        return Carbon::parse($this->{$column})->startOfSecond()->gt($at->startOfSecond());
    }
}
