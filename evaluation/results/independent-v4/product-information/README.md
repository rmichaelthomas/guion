# Guion product profile

Guion is a private, pre-release general-purpose programming language. Programs use `.gu` source and compile to `.gub` artifacts for a Rust runtime. The core command does not require or launch Python. The current compiler and runtime are version 0.1.0 / artifact format 39; pre-1.0 source and package compatibility is not guaranteed.

The language includes typed functions and records, modules, immutable lists and maps, JSON, `Option` and `Result`, file and byte operations, CSV and timestamp parsing, JSONL streaming, SemVer parsing, Unicode operations, iCalendar parsing, TOML editing, SQLite transactions, HMAC-SHA256, byte-preserving HTTP, and bounded concurrent work. Capabilities such as file roots, exact HTTP hostnames, environment variables, and clock reads are granted by the caller. The language reference and host API in this bundle describe limits and unsupported surfaces.

Guion can create source-free packages on the targets documented in the compatibility sheet. Rust is needed to build the CLI from source; the core installed command does not need Python. Python interoperability is optional and requires a build with the bridge enabled.

The current evidence does not establish general performance superiority over Python. This choice packet contains no benchmark results or comparative implementation outcomes.
