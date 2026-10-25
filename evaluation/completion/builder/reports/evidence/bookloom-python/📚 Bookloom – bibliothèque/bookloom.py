#!/usr/bin/env python3
"""Bookloom: a small, local SQLite lending catalog."""

from __future__ import annotations

import argparse
import re
import sqlite3
import sys
import tomllib
from contextlib import contextmanager
from datetime import date, timedelta
from pathlib import Path
from typing import Iterator


class CatalogError(Exception):
    """An expected catalog or configuration error suitable for CLI output."""


def load_config(path: Path) -> tuple[int, Path]:
    try:
        with path.open("rb") as config_file:
            data = tomllib.load(config_file)
    except OSError as exc:
        raise CatalogError(f"cannot read config '{path}': {exc.strerror or exc}") from exc
    except tomllib.TOMLDecodeError as exc:
        raise CatalogError(f"invalid TOML config '{path}': {exc}") from exc

    checkout_days = data.get("checkout_days")
    if isinstance(checkout_days, bool) or not isinstance(checkout_days, int) or checkout_days < 1:
        raise CatalogError("config checkout_days must be a positive integer")

    database_value = data.get("database_path")
    if not isinstance(database_value, str) or not database_value.strip():
        raise CatalogError("config database_path must be a non-empty path")
    database_path = Path(database_value).expanduser()
    if not database_path.is_absolute():
        database_path = path.resolve().parent / database_path
    return checkout_days, database_path


def connect(database_path: Path) -> sqlite3.Connection:
    try:
        database_path.parent.mkdir(parents=True, exist_ok=True)
        connection = sqlite3.connect(database_path, timeout=5.0, isolation_level=None)
        connection.row_factory = sqlite3.Row
        connection.execute(
            """CREATE TABLE IF NOT EXISTS books (
                book_id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                author TEXT NOT NULL,
                checked_out_to TEXT,
                checked_out_on TEXT,
                due_on TEXT,
                CHECK (
                    (checked_out_to IS NULL AND checked_out_on IS NULL AND due_on IS NULL)
                    OR
                    (checked_out_to IS NOT NULL AND checked_out_on IS NOT NULL AND due_on IS NOT NULL)
                )
            )"""
        )
        return connection
    except (OSError, sqlite3.Error) as exc:
        raise CatalogError(f"cannot open database '{database_path}': {exc}") from exc


@contextmanager
def write_transaction(connection: sqlite3.Connection) -> Iterator[None]:
    connection.execute("BEGIN IMMEDIATE")
    try:
        yield
    except BaseException:
        connection.rollback()
        raise
    else:
        connection.commit()


def add_book(database_path: Path, book_id: str, title: str, author: str) -> None:
    if not book_id.strip() or not title.strip():
        raise CatalogError("book ID and title must not be empty")
    connection = connect(database_path)
    try:
        try:
            with write_transaction(connection):
                connection.execute(
                    "INSERT INTO books (book_id, title, author) VALUES (?, ?, ?)",
                    (book_id, title, author),
                )
        except sqlite3.IntegrityError as exc:
            if "books.book_id" in str(exc) or "UNIQUE constraint failed" in str(exc):
                raise CatalogError(f"book ID '{book_id}' already exists") from exc
            raise CatalogError(f"cannot add book: {exc}") from exc
    finally:
        connection.close()


def checkout_book(database_path: Path, checkout_days: int, book_id: str, borrower: str) -> None:
    if not borrower.strip():
        raise CatalogError("borrower must not be empty")
    today = date.today()
    due_on = today + timedelta(days=checkout_days)
    connection = connect(database_path)
    try:
        with write_transaction(connection):
            book = connection.execute(
                "SELECT checked_out_to FROM books WHERE book_id = ?", (book_id,)
            ).fetchone()
            if book is None:
                raise CatalogError(f"unknown book ID '{book_id}'")
            if book["checked_out_to"] is not None:
                raise CatalogError(
                    f"book ID '{book_id}' is already checked out to {book['checked_out_to']}"
                )
            connection.execute(
                """UPDATE books
                   SET checked_out_to = ?, checked_out_on = ?, due_on = ?
                   WHERE book_id = ?""",
                (borrower, today.isoformat(), due_on.isoformat(), book_id),
            )
    finally:
        connection.close()


def return_book(database_path: Path, book_id: str) -> None:
    connection = connect(database_path)
    try:
        with write_transaction(connection):
            book = connection.execute(
                "SELECT checked_out_to FROM books WHERE book_id = ?", (book_id,)
            ).fetchone()
            if book is None:
                raise CatalogError(f"unknown book ID '{book_id}'")
            if book["checked_out_to"] is None:
                raise CatalogError(f"book ID '{book_id}' is not checked out")
            connection.execute(
                """UPDATE books
                   SET checked_out_to = NULL, checked_out_on = NULL, due_on = NULL
                   WHERE book_id = ?""",
                (book_id,),
            )
    finally:
        connection.close()


def list_books(database_path: Path, status: str) -> list[sqlite3.Row]:
    connection = connect(database_path)
    try:
        if status == "available":
            query = """SELECT book_id, title, author FROM books
                       WHERE checked_out_to IS NULL
                       ORDER BY book_id COLLATE NOCASE, book_id"""
        else:
            query = """SELECT book_id, title, author, checked_out_to, due_on FROM books
                       WHERE checked_out_to IS NOT NULL
                       ORDER BY book_id COLLATE NOCASE, book_id"""
        return list(connection.execute(query))
    finally:
        connection.close()


def parse_iso_date(value: str) -> date:
    if re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", value) is None:
        raise CatalogError(f"invalid date '{value}'; expected YYYY-MM-DD")
    try:
        return date.fromisoformat(value)
    except ValueError as exc:
        raise CatalogError(f"invalid date '{value}'; expected a real calendar date") from exc


def list_due_soon(database_path: Path, as_of: date, checkout_days: int) -> list[sqlite3.Row]:
    try:
        through = as_of + timedelta(days=checkout_days)
    except OverflowError as exc:
        raise CatalogError("as-of date is too late to form the checkout window") from exc
    connection = connect(database_path)
    try:
        return list(
            connection.execute(
                """SELECT book_id, title, author, checked_out_to, due_on FROM books
                   WHERE checked_out_to IS NOT NULL AND due_on >= ? AND due_on <= ?
                   ORDER BY due_on, book_id COLLATE NOCASE, book_id""",
                (as_of.isoformat(), through.isoformat()),
            )
        )
    finally:
        connection.close()


def render_list(rows: list[sqlite3.Row], status: str) -> None:
    if not rows:
        print(f"No {status} books.")
        return
    for row in rows:
        if status == "available":
            print(f"{row['book_id']} | {row['title']} | {row['author']}")
        else:
            print(
                f"{row['book_id']} | {row['title']} | {row['author']} | "
                f"borrower: {row['checked_out_to']} | due: {row['due_on']}"
            )


def render_due_soon(rows: list[sqlite3.Row], as_of: date, checkout_days: int) -> None:
    if not rows:
        print(f"No books due from {as_of.isoformat()} through {as_of + timedelta(days=checkout_days)}.")
        return
    for row in rows:
        print(
            f"{row['due_on']} | {row['book_id']} | {row['title']} | "
            f"{row['author']} | borrower: {row['checked_out_to']}"
        )


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="bookloom", description="Local lending catalog")
    parser.add_argument(
        "--config",
        type=Path,
        default=Path(__file__).resolve().with_name("bookloom.toml"),
        help="TOML config path (default: bookloom.toml beside this program)",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    add = commands.add_parser("add", help="add an available book")
    add.add_argument("--id", required=True, dest="book_id")
    add.add_argument("--title", required=True)
    add.add_argument("--author", default="")

    checkout = commands.add_parser("checkout", help="check out a book to a borrower")
    checkout.add_argument("--id", required=True, dest="book_id")
    checkout.add_argument("--borrower", required=True)

    returning = commands.add_parser("return", help="return a checked-out book")
    returning.add_argument("--id", required=True, dest="book_id")

    listing = commands.add_parser("list", help="list books by availability")
    listing.add_argument("status", choices=("available", "checked-out"))

    due_soon = commands.add_parser("due-soon", help="list books due within the checkout window")
    due_soon.add_argument("--as-of", required=True, metavar="YYYY-MM-DD")
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        checkout_days, database_path = load_config(args.config)
        if args.command == "add":
            add_book(database_path, args.book_id, args.title, args.author)
            print(f"Added '{args.book_id}'.")
        elif args.command == "checkout":
            checkout_book(database_path, checkout_days, args.book_id, args.borrower)
            print(f"Checked out '{args.book_id}' to {args.borrower}.")
        elif args.command == "return":
            return_book(database_path, args.book_id)
            print(f"Returned '{args.book_id}'.")
        elif args.command == "list":
            render_list(list_books(database_path, args.status), args.status)
        elif args.command == "due-soon":
            as_of = parse_iso_date(args.as_of)
            rows = list_due_soon(database_path, as_of, checkout_days)
            render_due_soon(rows, as_of, checkout_days)
        return 0
    except CatalogError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 2
    except sqlite3.Error as exc:
        print(f"ERROR: database operation failed: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
