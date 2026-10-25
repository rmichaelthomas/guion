# Bookloom

Bookloom is a local lending catalog written in Veritas. It uses Veritas's native TOML and SQLite services; its database is stored in the caller-selected writable file root.

## Configuration

Edit bookloom.toml. Set checkout_days to a positive number and database to a filename. The sample uses 14 days and bookloom.sqlite. The selected data directory must already exist.

## Commands

Build and run the source project with the installed Veritas distribution:

    veritas check src/main.vr
    veritas run --file-root /path/to/book-data . add B001 "The Left Hand of Darkness"
    veritas run --file-root /path/to/book-data . checkout B001 "A. Reader" 2026-10-25
    veritas run --file-root /path/to/book-data . list
    veritas run --file-root /path/to/book-data . list available
    veritas run --file-root /path/to/book-data . list checked-out
    veritas run --file-root /path/to/book-data . due-soon --as-of 2026-11-01
    veritas run --file-root /path/to/book-data . return B001

Checkout uses an explicit ISO date. due-soon includes books due on or after the supplied date and no later than checkout_days after it. Results sort by due date then book ID. Invalid dates are rejected before SQLite is opened. Duplicate IDs, unknown IDs, and checkout conflicts produce clear errors; state changes use SQLite transactions and roll back on failure.

## Package and install

    veritas package /path/to/bookloom --output /path/to/bookloom-package
    veritas verify-package /path/to/bookloom-package
    veritas install /path/to/bookloom-package --prefix /path/to/bookloom-install

For the installed command, run it while your working directory is the writable data directory. This keeps the SQLite file separate from the installed application:

    cd /path/to/book-data
    /path/to/bookloom-install/bin/bookloom due-soon --as-of 2026-11-01
