**python**

Comment-preserving TOML editing is the decisive requirement. Python can use a maintained library directly, provided it is in the pinned offline wheelhouse. Veritas’s supplied profile does not establish a native comment-preserving TOML editing API; using its Python bridge would add adapters, declarations, and a second runtime to maintain for this small application.

Python still requires careful validation: explicitly reject booleans as integers, validate reserved fields before modifying anything, advance only one version, and preserve original bytes on failure and current-version no-ops. Atomic replacement must follow successful validation and serialization. Neither language guarantees these application behaviors automatically.

Delivery is a condition of this recommendation: the Python artifact must include or explicitly provision its managed runtime and dependencies so relocation does not depend implicitly on system Python. A wheel alone does not establish that. Veritas provides an explicit managed-Python bundling workflow, which is a meaningful advantage.

I would reconsider if an existing Veritas adapter already handled comment-preserving TOML reliably and its supported-target packaging substantially reduced delivery work. If the required TOML library is absent from the wheelhouse, reassess both approaches; Veritas interop cannot bypass that restriction. Startup time, package size, and dependency-resolution cost remain unmeasured.
