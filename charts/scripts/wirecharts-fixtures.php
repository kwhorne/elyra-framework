<?php
/**
 * WireCharts' options for every chart in its docs (RFC 0059), as the fixtures
 * @elyra/charts is held to:
 *
 *   php charts/scripts/wirecharts-fixtures.php ../wirecharts ../wirecharts-site \
 *     > charts/src/test/wirecharts/fixtures.json
 *
 * For each component in the site's catalog, the example's props (evaluated
 * as Blade would) and the option WireCharts renders for them, read out of the
 * rendered HTML. Functions (WireCharts' "@@…@@" strings) become "@@function@@":
 * Elyra's are real functions, compared as such.
 */

[$_, $wirecharts, $site] = $argv + [null, null, null];
if (! $wirecharts || ! $site) {
    fwrite(STDERR, "usage: php wirecharts-fixtures.php <wirecharts> <wirecharts-site>\n");
    exit(2);
}

// line-boost's option is long: the pattern reads it whole.
ini_set('pcre.backtrack_limit', '100000000');

require $wirecharts.'/vendor/autoload.php';
require $site.'/app/Docs/Catalog.php';

use Illuminate\Support\Facades\Blade;
use WireCharts\Licensing\License;
use WireCharts\WireChartsServiceProvider;

$app = Orchestra\Testbench\Foundation\Application::create(
    basePath: null,
    options: ['extra' => ['providers' => [WireChartsServiceProvider::class]]],
);
// Rendering, not licensing: every chart unlocked.
$app->instance(License::class, new class extends License
{
    public function allows(string $component): bool
    {
        return true;
    }
});

/**
 * What the site's views hand an example that names a variable: line-boost's
 * `$points`, a long signal, made the same way every time.
 */
function vars(): array
{
    return ['points' => array_map(fn ($i) => round(sin($i / 20) * 40 + 50 + sin($i / 3) * 5, 2), range(0, 1999))];
}

/** `:name="php"`, `name="text"` and bare `name`, as Blade passes them, camel-cased. */
function propsOf(string $tag): array
{
    extract(vars());
    $props = [];
    preg_match_all('/\s(:?)([a-zA-Z][\w-]*)(?:="((?:[^"\\\\]|\\\\.)*)")?/s', $tag, $m, PREG_SET_ORDER | PREG_UNMATCHED_AS_NULL);
    foreach ($m as $match) {
        [$all, $bound, $name] = $match;
        $value = $match[3] ?? null;
        $key = lcfirst(str_replace(' ', '', ucwords(str_replace('-', ' ', $name))));
        if ($bound === ':') {
            $props[$key] = eval('return '.$value.';');
        } elseif ($value === null) {
            $props[$key] = true;
        } else {
            $props[$key] = is_numeric($value) ? $value + 0 : $value;
        }
    }

    return $props;
}

/** The JSON each `JSON.parse('…')` in the chart's x-data carries. */
function optionsIn(string $html): array
{
    preg_match('/x-data="(wireChart\w*)\((.*?)\)"\s/s', $html, $m);
    // `@js` escapes every quote, so each literal ends at the first `')`.
    $decoded = [];
    foreach (array_slice(explode("JSON.parse('", $m[2] ?? ''), 1) as $part) {
        $js = substr($part, 0, strpos($part, "')"));
        // As objects: an empty `{}` (PHP's `(object) []`) must stay one.
        $decoded[] = json_decode(json_decode('"'.$js.'"'));
    }

    return [$m[1] ?? null, $decoded];
}

function functions(mixed $node): mixed
{
    if (is_array($node)) {
        return array_map('functions', $node);
    }
    if ($node instanceof stdClass) {
        $out = new stdClass;
        foreach (get_object_vars($node) as $key => $value) {
            $out->{$key} = functions($value);
        }

        return $out;
    }
    if (is_string($node) && str_starts_with($node, '@@') && str_ends_with($node, '@@')) {
        return '@@function@@';
    }

    return $node;
}

/** A component's own defaults: its `@props([...])`, evaluated. */
function defaultsOf(string $wirecharts, string $slug): array
{
    $blade = file_get_contents("{$wirecharts}/resources/views/components/{$slug}.blade.php");
    if (! preg_match('/@props\((\[.*?\])\)\s*\n/s', $blade, $m)) {
        return [];
    }
    $props = eval('return '.$m[1].';');
    $defaults = [];
    foreach ($props as $key => $value) {
        // `['series' => []]` has a default; a bare `'series'` has none.
        if (is_int($key)) {
            $defaults[$value] = null;
        } else {
            $defaults[$key] = $value;
        }
    }

    return $defaults;
}

/** The Option.php builder a component's view calls. */
function builderOf(string $wirecharts, string $slug): ?string
{
    $blade = file_get_contents("{$wirecharts}/resources/views/components/{$slug}.blade.php");

    return preg_match('/Option::(\w+)\(/', $blade, $m) ? $m[1] : null;
}

/** A builder's source, and that of the builders it calls, as text. */
function sourceOf(string $builder, array &$seen = []): string
{
    if (isset($seen[$builder]) || ! method_exists(WireCharts\Support\Option::class, $builder)) {
        return '';
    }
    $seen[$builder] = true;
    $method = new ReflectionMethod(WireCharts\Support\Option::class, $builder);
    $lines = file($method->getFileName());
    $body = implode('', array_slice($lines, $method->getStartLine() - 1, $method->getEndLine() - $method->getStartLine() + 1));
    preg_match_all('/(?:static|self)::(\w+)\(/', $body, $calls);
    foreach (array_unique($calls[1]) as $called) {
        $body .= sourceOf($called, $seen);
    }

    return $body;
}

/**
 * The ECharts parts a chart can need: its series types and the components
 * its builder writes (a mark line, a data zoom), read from the builder's
 * source and the example's option, so a prop the example leaves off counts.
 */
function partsOf(?string $builder, mixed $option): array
{
    $text = ($builder ? sourceOf($builder) : '').json_encode($option);
    $series = ['line', 'bar', 'pie', 'scatter', 'effectScatter', 'radar', 'funnel', 'gauge', 'heatmap',
        'candlestick', 'boxplot', 'tree', 'treemap', 'sunburst', 'sankey', 'graph', 'themeRiver',
        'pictorialBar', 'custom', 'parallel', 'lines', 'bar3D', 'scatter3D', 'surface', 'line3D'];
    $charts = array_values(array_filter($series, fn ($t) => preg_match("/['\"]type['\"]\s*(=>|:)\s*['\"]{$t}['\"]/", $text)));
    $components = ['markLine', 'markArea', 'markPoint', 'dataZoom', 'visualMap', 'title', 'graphic', 'polar',
        'singleAxis', 'calendar', 'dataset', 'toolbox', 'timeline', 'brush', 'parallelAxis', 'labelLayout',
        'universalTransition', 'grid3D', 'xAxis3D', 'radar'];
    $used = array_values(array_filter($components, fn ($c) => preg_match("/['\"]{$c}['\"]/", $text)));

    return ['charts' => $charts, 'components' => $used];
}

$out = [];
foreach (App\Docs\Catalog::categories() as $category) {
    foreach ($category['components'] as $component) {
        $example = $component['example'];
        preg_match('/<chart:([\w-]+)(.*?)\/>/s', $example, $tag);
        $html = Blade::render($example, vars());
        [$glue, $options] = optionsIn($html);
        $out[] = [
            'component' => $component['slug'],
            'family' => $category['title'],
            'name' => $component['name'],
            'blurb' => $component['blurb'],
            'example' => $example,
            'props' => propsOf($tag[2] ?? ''),
            'defaults' => defaultsOf($wirecharts, $component['slug']),
            'builder' => builderOf($wirecharts, $component['slug']),
            'parts' => partsOf(builderOf($wirecharts, $component['slug']), $options[0] ?? null),
            'docProps' => $component['props'],
            'glue' => $glue,
            'option' => functions($options[0] ?? null),
            'config' => functions($options[1] ?? null),
        ];
    }
}

echo json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION), "\n";
