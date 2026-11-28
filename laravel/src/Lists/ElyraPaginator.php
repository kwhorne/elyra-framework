<?php

namespace Elyra\Backend\Lists;

use Illuminate\Pagination\LengthAwarePaginator;

/**
 * Laravel's paginator, with the list's totals beside `data` (RFC 0071):
 * what an Elyra `Table` reads for its foot. Under an API Resource collection
 * they're in `meta`, which the desktop reads too.
 */
class ElyraPaginator extends LengthAwarePaginator
{
    public function __construct($items, $total, $perPage, $currentPage = null, array $options = [], private ?array $totals = null)
    {
        parent::__construct($items, $total, $perPage, $currentPage, $options);
    }

    /** The totals by column, or null when the list asked for none. */
    public function totals(): ?array
    {
        return $this->totals;
    }

    public function toArray(): array
    {
        $array = parent::toArray();
        if ($this->totals !== null) {
            $array['totals'] = $this->totals;
        }

        return $array;
    }
}
