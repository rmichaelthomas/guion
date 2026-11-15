# Guion for VS Code

Syntax colouring, plus errors as you type, hover, go to definition, completion and format
document, all from the language server built into the `guion` program (`guion lsp`).

1. Install `guion` and check that `guion --version` works in a terminal.
2. In this folder run `npm install`, then open the folder in VS Code and press F5 to try it, or
   package it with `vsce package`.
3. If `guion` is not on your path, set `guion.executable` in the settings.

The server reads your buffer and the files it imports; it never runs your program.
