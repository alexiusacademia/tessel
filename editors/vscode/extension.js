// Starts `tessel lsp`, which gives VS Code Tessel's errors, types on hover and
// completion, and adds commands that run and build the program being edited.
const fs = require("fs");
const path = require("path");
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client;

/// The open folder, where a project's tessel.toml usually is.
function workspaceFolder() {
  return vscode.workspace.workspaceFolders?.[0]?.uri.fsPath ?? "";
}

/// The `tessel` command, from the `tessel.path` setting.
function tesselCommand() {
  return (vscode.workspace.getConfiguration("tessel").get("path") || "tessel").replace(
    "${workspaceFolder}",
    workspaceFolder()
  );
}

/// The program the active editor's file belongs to: its folder when that's
/// a program (it has a main.tsl or a tessel.toml), or else the file, as a
/// program of its own.
function currentProgram() {
  const doc = vscode.window.activeTextEditor?.document;
  if (!doc || doc.languageId !== "tessel" || doc.uri.scheme !== "file") {
    vscode.window.showWarningMessage("Tessel: open a .tsl file of the program first.");
    return undefined;
  }
  const folder = path.dirname(doc.uri.fsPath);
  const project = ["main.tsl", "tessel.toml"].some((f) => fs.existsSync(path.join(folder, f)));
  return project ? folder : doc.uri.fsPath;
}

/// The folder to run a program from: its own, or its file's.
function folderOf(program) {
  return fs.existsSync(program) && fs.statSync(program).isDirectory() ? program : path.dirname(program);
}

/// Runs `tessel <args>` in a terminal of its own, in `cwd`.
function runInTerminal(name, cwd, args) {
  const quote = (s) => (/^[A-Za-z0-9_.\/:+=-]+$/.test(s) ? s : JSON.stringify(s));
  let terminal = vscode.window.terminals.find((t) => t.name === name);
  if (terminal) {
    // (A terminal used before may be in another folder.)
    terminal.sendText(`cd ${quote(cwd)}`);
  } else {
    terminal = vscode.window.createTerminal({ name, cwd });
  }
  terminal.show(true);
  terminal.sendText([quote(tesselCommand()), ...args.map(quote)].join(" "));
}

/// Saves the program's files first, so it runs as they show.
async function saveAndRun(name, args) {
  const program = currentProgram();
  if (!program) {
    return;
  }
  await vscode.workspace.saveAll(false);
  runInTerminal(name, folderOf(program), args(program));
}

function startClient() {
  const command = tesselCommand();
  client = new LanguageClient(
    "tessel",
    "Tessel",
    // In the project's folder, so its tessel.toml (the version of Tessel it
    // uses) applies.
    { command, args: ["lsp"], options: { cwd: workspaceFolder() || undefined } },
    { documentSelector: [{ scheme: "file", language: "tessel" }] }
  );
  client.start().catch((err) => {
    vscode.window.showErrorMessage(
      `Tessel: couldn't start \`${command} lsp\` (${err.message}). Set "tessel.path" to the tessel executable.`
    );
  });
}

function activate(context) {
  startClient();
  const commands = {
    "tessel.run": () => saveAndRun("Tessel", (p) => ["run", p]),
    "tessel.runHot": () => saveAndRun("Tessel", (p) => ["run", p, "--hot"]),
    "tessel.build": () => saveAndRun("Tessel", (p) => ["build", p, "--bundle"]),
    "tessel.check": () => saveAndRun("Tessel", (p) => ["check", p]),
    "tessel.pin": () => {
      const program = currentProgram();
      const folder = program ? folderOf(program) : workspaceFolder();
      if (folder) {
        runInTerminal("Tessel", folder, ["pin"]);
      }
    },
    "tessel.restart": async () => {
      await client?.stop();
      startClient();
    },
  };
  for (const [id, run] of Object.entries(commands)) {
    context.subscriptions.push(vscode.commands.registerCommand(id, run));
  }
  // A changed tessel.toml may ask for another version of Tessel: the
  // language server starts again with it.
  const watcher = vscode.workspace.createFileSystemWatcher("**/tessel.toml");
  const restart = () => commands["tessel.restart"]();
  watcher.onDidChange(restart);
  watcher.onDidCreate(restart);
  watcher.onDidDelete(restart);
  context.subscriptions.push(watcher);
  // So does a changed `tessel.path`.
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("tessel.path")) {
        restart();
      }
    })
  );
  context.subscriptions.push({ dispose: () => client && client.stop() });
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
