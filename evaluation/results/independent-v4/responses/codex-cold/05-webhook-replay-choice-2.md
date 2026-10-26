**Python.**

This task fits Python’s standard library closely: `argparse` for commands, `sqlite3` for durable state, `hmac` and `hashlib` for authentication, `json` for validation and exports, and HTTP support for delivery. That avoids requiring additional downloads under the offline constraint.

The decisive details are exact byte handling and persisted recovery state. Python can retain the original body bytes, verify decoded HMAC digests before parsing JSON, distinguish a missing `payload` from `null`, and use database transactions and unique IDs to prevent rejected ingests from changing state. Persisted attempt counts and explicit pending, delivered, and dead-letter states support the follow-up policy across restarts. These are implementation capabilities, not verified results; transaction boundaries and network-failure handling still require careful design and testing.

With no supplied Guion profile and insufficient prior knowledge of its relevant facilities, I cannot establish an equally direct path for its storage, cryptography, HTTP, and source-free execution requirements.

I would reconsider if Guion demonstrated those capabilities—or a dependable managed Python bridge—with straightforward byte preservation, transactional updates, and offline packaging. For this brief, Python offers the clearer implementation path with fewer unknowns.
