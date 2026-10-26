**Python.** For this task, I would prioritize mature integration and maintenance over Guion’s packaging and capability controls.

The difficult part is preserving the replay state machine: reject invalid input without database mutation, authenticate exact bytes before parsing, distinguish a missing payload from `null`, and persist attempt counts correctly across restarts. The follow-up adds a particularly sensitive boundary: an event already at the attempt limit must become dead-lettered without another request. Python’s supplied profile supports choosing a mature general-purpose language with a broad standard library for this integration-heavy CLI. This is a risk judgment, not evidence of a successful implementation.

Guion is a credible alternative here. Its documented SQLite transactions, HMAC verification, byte-preserving HTTP, JSON presence checks, and explicit error types directly match the requirements. Its source-free packaging is also relevant to acceptance. However, its pre-1.0 compatibility uncertainty and relative-path/file-root rules add deployment considerations for commands accepting database, secret, body, and output paths. Network host grants also need configuration for the supplied loopback target.

Neither language automatically guarantees secret privacy, correct transaction boundaries, or durable retry accounting. Those require deliberate implementation and acceptance testing.

I would change my choice to **Guion** if deployment strongly favored its documented source-free bundles and caller-granted capabilities, the target platform was supported, and the team accepted its version pinning and path restrictions. Python’s clean source-free delivery would still need to be established; the supplied profile does not demonstrate it.
