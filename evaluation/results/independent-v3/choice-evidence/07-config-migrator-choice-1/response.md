**python**

The decisive requirement is comment-preserving TOML editing. Python can use a maintained library directly, provided it exists in the pinned offline wheelhouse. The supplied Veritas profile documents no native comment-preserving TOML API; its managed Python bridge offers a route, but adds adapter code and a second runtime to maintain for this small application.

Python still requires careful validation: reject booleans as integers, validate reserved fields before modifying anything, preserve explicit values, and perform atomic replacement only after successful validation. Neither language automatically guarantees these application rules.

Veritas has a concrete delivery advantage: its documented package workflow bundles managed CPython, locked dependencies, and an offline cache. Python’s required wheel or zipapp does not itself establish interpreter independence; delivery must explicitly satisfy the “no implicit system-Python dependency” requirement. Startup time, package size, and API usability remain unmeasured for both options.

I would reconsider if the wheelhouse lacks a suitable TOML library, Veritas gains a suitable native editing API, or its supported bundling workflow becomes the overriding deployment requirement. Missing TOML dependencies would require reassessing both candidates, since Veritas interop shares the same wheelhouse restriction.
