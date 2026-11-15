// Starts `guion lsp` and connects the editor to it.
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function activate(context) {
  const executable = vscode.workspace.getConfiguration("guion").get("executable", "guion");
  client = new LanguageClient(
    "guion",
    "Guion",
    { command: executable, args: ["lsp"] },
    { documentSelector: [{ scheme: "file", language: "guion" }] }
  );
  client.start();
  context.subscriptions.push({ dispose: () => client && client.stop() });
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
