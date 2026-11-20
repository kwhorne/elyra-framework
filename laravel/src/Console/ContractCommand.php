<?php

namespace Elyra\Backend\Console;

use Illuminate\Console\Command;
use Illuminate\Contracts\Http\Kernel;
use Illuminate\Foundation\Http\FormRequest;
use Illuminate\Http\Request;
use Illuminate\Routing\Route;
use Illuminate\Support\Facades\Route as Routes;

/**
 * php artisan elyra:contract --as=ada@example.com > elyra-contract.json
 *
 * The API as the desktop app sees it (RFC 0040): the routes under /api, the
 * rules of each route's FormRequest, the abilities there are, and the shape
 * of what each GET answers, sampled as the user --as names: each field's
 * type, and whether it was ever null. Names and types only, never a value.
 * `rata check --backend elyra-contract.json` compares the desktop with it.
 */
class ContractCommand extends Command
{
    protected $signature = 'elyra:contract
        {--as= : The user (email) whose answers are sampled; without it, routes, rules and abilities only}
        {--prefix=api : The routes the desktop calls}
        {--samples=25 : The most records a list is sampled with}';

    protected $description = "Describe the API for the desktop app's rata check --backend (RFC 0040)";

    public function handle(): int
    {
        $prefix = trim((string) $this->option('prefix'), '/');
        $routes = collect(Routes::getRoutes()->getRoutes())
            ->filter(fn (Route $route) => $prefix === '' || str_starts_with($route->uri(), $prefix.'/') || $route->uri() === $prefix)
            ->flatMap(fn (Route $route) => collect($route->methods())
                ->reject(fn (string $method) => in_array($method, ['HEAD', 'OPTIONS'], true))
                ->map(fn (string $method) => [
                    'method' => $method,
                    'uri' => '/'.ltrim($route->uri(), '/'),
                    'middleware' => array_values(array_map('strval', $route->gatherMiddleware())),
                    'rules' => $method === 'GET' ? null : $this->rules($route, $method),
                ]))
            ->sortBy(fn (array $r) => $r['uri'].' '.$r['method'])
            ->values()
            ->all();

        $user = null;
        if ($email = $this->option('as')) {
            $model = config('auth.providers.users.model', 'App\\Models\\User');
            $user = $model::where('email', $email)->first();
            if (! $user) {
                $this->error("No user {$email}.");

                return self::FAILURE;
            }
            if (! method_exists($user, 'createToken')) {
                $this->error("{$model} has no createToken(): add Sanctum's HasApiTokens.");

                return self::FAILURE;
            }
            $routes = $this->sampled($routes, $user, max(1, (int) $this->option('samples')));
        }

        $this->line(json_encode([
            'laravel' => app()->version(),
            'sampled_as' => $user ? $email : null,
            'routes' => $routes,
            'abilities' => $this->abilities(),
        ], JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES));

        return self::SUCCESS;
    }

    /**
     * The rules of the FormRequest the route's action takes, as rule strings;
     * null when it takes none (inline $request->validate() can't be read
     * without running it) or its rules can't be made here.
     */
    private function rules(Route $route, string $method): ?array
    {
        $action = $route->getActionName();
        if (! str_contains($action, '@')) {
            return null;
        }
        [$class, $name] = explode('@', $action, 2);
        if (! method_exists($class, $name)) {
            return null;
        }
        foreach ((new \ReflectionMethod($class, $name))->getParameters() as $parameter) {
            $type = $parameter->getType();
            if (! $type instanceof \ReflectionNamedType || $type->isBuiltin()) {
                continue;
            }
            $requestClass = $type->getName();
            if (! is_subclass_of($requestClass, FormRequest::class)) {
                continue;
            }
            try {
                $request = $requestClass::createFrom(Request::create('/'.$route->uri(), $method));
                $request->setContainer(app())->setRedirector(app('redirect'));
                $rules = method_exists($request, 'rules') ? app()->call([$request, 'rules']) : [];
            } catch (\Throwable) {
                return null;
            }

            return collect($rules)->map(fn ($rule) => $this->ruleString($rule))->all();
        }

        return null;
    }

    private function ruleString(mixed $rule): string
    {
        $parts = is_array($rule) ? $rule : explode('|', (string) $rule);

        return collect($parts)
            ->map(fn ($part) => is_string($part) ? $part : class_basename($part))
            ->implode('|');
    }

    /**
     * Each GET route's answer, asked through the kernel as `$user`, with a
     * token made for it and deleted after. A route with one parameter takes
     * the first id of its list's answer; one that can't be asked is left
     * without an answer.
     */
    private function sampled(array $routes, $user, int $samples): array
    {
        $token = $user->createToken('elyra-contract');
        $bearer = $token->plainTextToken;
        $firstIds = [];
        try {
            // Lists first, so a record's route can take an id from its list.
            $order = collect($routes)->sortBy(fn (array $r) => substr_count($r['uri'], '{'))->keys();
            foreach ($order as $i) {
                $route = $routes[$i];
                if ($route['method'] !== 'GET') {
                    continue;
                }
                $uri = $route['uri'];
                if (preg_match_all('/\{(\w+)\??\}/', $uri, $m) === 1) {
                    $list = substr($uri, 0, strrpos($uri, '/'));
                    if (! isset($firstIds[$list])) {
                        continue;
                    }
                    $uri = str_replace($m[0][0], (string) $firstIds[$list], $uri);
                } elseif (str_contains($uri, '{')) {
                    continue;
                }
                $answer = $this->answerOf($uri, $bearer, $samples);
                if ($answer === null) {
                    continue;
                }
                [$shape, $records] = $answer;
                $routes[$i]['answer'] = $shape;
                if (in_array($shape['kind'], ['page', 'list'], true) && isset($records[0]['id'])) {
                    $firstIds[$route['uri']] = $records[0]['id'];
                }
            }
        } finally {
            $token->accessToken->delete();
        }

        return $routes;
    }

    /** @return array{0: array, 1: array}|null */
    private function answerOf(string $uri, string $bearer, int $samples): ?array
    {
        $request = Request::create($uri, 'GET', ['per_page' => $samples]);
        $request->headers->set('Authorization', "Bearer {$bearer}");
        $request->headers->set('Accept', 'application/json');
        try {
            $response = app(Kernel::class)->handle($request);
        } catch (\Throwable) {
            return null;
        }
        if ($response->getStatusCode() !== 200) {
            return null;
        }
        $body = json_decode((string) $response->getContent(), true);
        if (! is_array($body)) {
            return null;
        }
        [$kind, $records] = match (true) {
            array_is_list($body) => ['list', $body],
            isset($body['data']) && is_array($body['data']) && array_is_list($body['data']) => ['page', $body['data']],
            isset($body['data']) && is_array($body['data']) => ['resource', [$body['data']]],
            default => ['record', [$body]],
        };
        $records = array_values(array_filter($records, 'is_array'));

        return [[
            'kind' => $kind,
            'fields' => $this->fields($records),
            'sampled' => count($records),
        ], $records];
    }

    /** Each field's types across the records, "string|null" when it was ever null. */
    private function fields(array $records): array
    {
        $fields = [];
        foreach ($records as $record) {
            foreach ($record as $name => $value) {
                $fields[$name][$this->type($value)] = true;
            }
        }
        // A field a record didn't have counts as null in it.
        foreach ($fields as $name => $types) {
            foreach ($records as $record) {
                if (! array_key_exists($name, $record)) {
                    $fields[$name]['null'] = true;
                }
            }
        }
        ksort($fields);

        return array_map(function (array $types) {
            $types = array_keys($types);
            usort($types, fn ($a, $b) => ($a === 'null') <=> ($b === 'null') ?: strcmp($a, $b));

            return implode('|', $types);
        }, $fields);
    }

    private function type(mixed $value): string
    {
        return match (true) {
            $value === null => 'null',
            is_bool($value) => 'bool',
            is_int($value) => 'int',
            is_float($value) => 'float',
            is_string($value) => 'string',
            is_array($value) && array_is_list($value) => 'array',
            default => 'object',
        };
    }

    /** What Route::elyraAbilities() answers for someone: every ability there is. */
    private function abilities(): array
    {
        $permission = 'Spatie\\Permission\\Models\\Permission';
        if (class_exists($permission)) {
            try {
                return $permission::query()->pluck('name')->unique()->sort()->values()->all();
            } catch (\Throwable) {
                // No permissions table: fall back to the config.
            }
        }

        return collect(config('elyra.abilities', []))->sort()->values()->all();
    }
}
