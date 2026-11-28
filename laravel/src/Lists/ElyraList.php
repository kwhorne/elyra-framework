<?php

namespace Elyra\Backend\Lists;

use Illuminate\Database\Eloquent\Builder;
use Illuminate\Http\Request;
use Illuminate\Support\Carbon;

/**
 * A desktop list's question, answered (RFC 0071): the search, the filters,
 * the sort and the page an Elyra `Table` sends, each through what the
 * controller allows, and the totals over every row that matched.
 *
 *   Invoice::query()->elyraList($request, [
 *       'search' => ['number', 'customer'],
 *       'sort' => ['number', 'due', 'amount'],
 *       'filters' => ['status' => 'options', 'amount' => 'range', 'due' => 'dates', 'customer' => 'text'],
 *       'totals' => ['amount' => 'sum'],
 *   ]);
 *
 * A key, a sort or a kind the options don't name is ignored; every value is
 * bound, never written into the SQL.
 */
class ElyraList
{
    private const KINDS = ['text', 'options', 'range', 'dates'];

    private const TOTALS = ['sum', 'avg', 'min', 'max', 'count'];

    public static function apply(Builder $query, Request $request, array $options = []): ElyraPaginator
    {
        $options += ['search' => [], 'sort' => [], 'filters' => [], 'totals' => [], 'per_page' => 25, 'max_per_page' => 100];
        $wrap = fn (string $column) => $query->getQuery()->getGrammar()->wrap($column);

        // The search: any of the columns contains the term, ignoring case.
        $term = trim((string) $request->query('search', ''));
        if ($term !== '' && $options['search']) {
            $query->where(function (Builder $q) use ($options, $term, $wrap) {
                foreach ($options['search'] as $column) {
                    $q->orWhereRaw('lower('.$wrap($column).") like lower(?) escape '!'", [self::contains($term)]);
                }
            });
        }

        // The filters, each as its kind says.
        $sent = $request->query('filters', []);
        if (is_array($sent)) {
            foreach ($options['filters'] as $column => $kind) {
                if (! in_array($kind, self::KINDS, true) || ! array_key_exists($column, $sent)) {
                    continue;
                }
                self::filter($query, $column, $kind, $sent[$column], $wrap);
            }
        }

        // The totals: over every row that matched, before the sort and the page.
        $totals = null;
        $asked = array_filter($options['totals'], fn ($fn) => in_array($fn, self::TOTALS, true));
        if ($asked) {
            $select = [];
            foreach ($asked as $column => $fn) {
                $select[] = "{$fn}(".$wrap($column).') as '.$wrap("elyra_total_{$column}");
            }
            $row = (clone $query)->toBase()->reorder()->selectRaw(implode(', ', $select))->first();
            $totals = [];
            foreach ($asked as $column => $fn) {
                $value = $row?->{"elyra_total_{$column}"};
                // Over no rows a sum and a count are 0; a mean, a least and a greatest aren't.
                if ($value !== null) {
                    $totals[$column] = (float) $value;
                } elseif ($fn === 'sum' || $fn === 'count') {
                    $totals[$column] = 0.0;
                }
            }
        }

        // The sort: a column the controller allows, else the key; then the key.
        $key = $query->getModel()->getKeyName();
        $sort = $request->query('sort');
        $column = is_string($sort) && in_array($sort, $options['sort'], true) ? $sort : $key;
        $query->orderBy($column, $request->query('direction') === 'desc' ? 'desc' : 'asc');
        if ($column !== $key) {
            $query->orderBy($key);
        }

        $perPage = max(1, min((int) $request->query('per_page', $options['per_page']), $options['max_per_page']));
        $page = $query->paginate($perPage);

        return new ElyraPaginator($page->items(), $page->total(), $page->perPage(), $page->currentPage(), $page->getOptions(), $totals);
    }

    /** `%term%`, its `%`, `_` and `!` as themselves (`escape '!'`). */
    private static function contains(string $term): string
    {
        return '%'.str_replace(['!', '%', '_'], ['!!', '!%', '!_'], $term).'%';
    }

    private static function filter(Builder $query, string $column, string $kind, mixed $value, callable $wrap): void
    {
        switch ($kind) {
            case 'text':
                if (is_string($value) && trim($value) !== '') {
                    $query->whereRaw('lower('.$wrap($column).") like lower(?) escape '!'", [self::contains(trim($value))]);
                }
                break;
            case 'options':
                $values = is_array($value) ? $value : (is_scalar($value) ? [$value] : []);
                if ($values && array_is_list($values) && count(array_filter($values, 'is_scalar')) === count($values)) {
                    $query->whereIn($column, $values);
                }
                break;
            case 'range':
                if (! is_array($value) || array_is_list($value)) {
                    break;
                }
                if (isset($value['min']) && is_numeric($value['min'])) {
                    $query->where($column, '>=', $value['min'] + 0);
                }
                if (isset($value['max']) && is_numeric($value['max'])) {
                    $query->where($column, '<=', $value['max'] + 0);
                }
                break;
            case 'dates':
                if (! is_array($value) || array_is_list($value)) {
                    break;
                }
                if (self::isDay($value['from'] ?? null)) {
                    $query->where($column, '>=', $value['from']);
                }
                if (self::isDay($value['to'] ?? null)) {
                    // Before the next day: a time on the last day is in.
                    $query->where($column, '<', Carbon::parse($value['to'])->addDay()->toDateString());
                }
                break;
        }
    }

    private static function isDay(mixed $value): bool
    {
        if (! is_string($value) || ! preg_match('/^(\d{4})-(\d{2})-(\d{2})$/', $value, $m)) {
            return false;
        }

        return checkdate((int) $m[2], (int) $m[3], (int) $m[1]);
    }
}
