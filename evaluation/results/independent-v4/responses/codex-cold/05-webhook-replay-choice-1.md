**Python.**

This task fits Python’s standard library closely: `argparse` for the CLI, `sqlite3` for durable state and unique IDs, `hmac`/`hashlib` for authentication, and HTTP and JSON support. That provides a credible route without additional downloads, which matters under the offline dependency restriction.

The decisive work is preserving exact behavior: validating the secret before database mutation, authenticating original bytes before JSON parsing, distinguishing missing payload from `null`, and persisting attempt counts and dead-letter transitions across processes. Python offers familiar primitives for expressing and testing those boundaries. This is a suitability judgment, not a claim that an implementation has passed acceptance.

Delivery still requires careful transaction design: a successful POST followed by a crash before recording success can cause replay. Choosing Python alone does not resolve that recovery boundary.

I lack enough prior knowledge of Guion to assess its persistence, byte handling, HTTP support, or managed bridge confidently. I would reconsider if Guion’s available environment established comparable support for these requirements and a simpler implementation or packaging path, including the clean source-free run. A bridge would also need to justify its extra integration boundary.
