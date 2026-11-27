Ported from WireCharts' `src/Support/Option.php` (RFC 0059), one builder per
function with the same name and arguments; `calls.js` maps each WireCharts
component to its builder as its Blade view does. `src/wirecharts.test.js`
holds every one to the option WireCharts renders for its docs' example
(`src/test/wirecharts/fixtures.json`, made by `scripts/wirecharts-fixtures.php`).
