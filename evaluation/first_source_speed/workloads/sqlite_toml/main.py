import sqlite3
import tomllib
from pathlib import Path

settings = tomllib.loads(Path("settings.toml").read_text(encoding="utf-8"))
with sqlite3.connect("agent-recipes.sqlite3") as database:
    database.execute("CREATE TABLE IF NOT EXISTS runs (name TEXT NOT NULL)")
    database.execute("INSERT INTO runs (name) VALUES (?)", (settings["app"]["name"],))
    row = database.execute("SELECT name FROM runs ORDER BY rowid DESC LIMIT 1").fetchone()
    print(row[0])
