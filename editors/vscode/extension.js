// Starts `tessel lsp`, which gives VS Code Tessel's errors, types on hover and completion.
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

function activate(context) {
  const folder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? "";
  const command = (vscode.workspace.getConfiguration("tessel").get("path") || "tessel").replace(
    "${workspaceFolder}",
    folder
  );
  client = new LanguageClient(
    "tessel",
    "Tessel",
    { command, args: ["lsp"] },
    { documentSelector: [{ scheme: "file", language: "tessel" }] }
  );
  client.start().catch((err) => {
    vscode.window.showErrorMessage(
      `Tessel: couldn't start \`${command} lsp\` (${err.message}). Set "tessel.path" to the tessel executable.`
    );
  });
  context.subscriptions.push({ dispose: () => client && client.stop() });
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
