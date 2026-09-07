// Client VS Code pour Rava : lance `rava-lsp` et lui parle en LSP.
//
// L'extension ne fait aucune analyse : tout vient du serveur, qui partage le
// lexer, le parser et le générateur de `ravac`. Un seul endroit dit la vérité.

const { workspace, window, commands } = require('vscode');
const { LanguageClient, TransportKind } = require('vscode-languageclient/node');
const { execFile } = require('child_process');
const path = require('path');

let client;

function serverPath() {
  const configured = workspace.getConfiguration('rava').get('server.path', 'rava-lsp');
  const folder = workspace.workspaceFolders && workspace.workspaceFolders[0];
  if (folder) {
    return configured.replace('${workspaceFolder}', folder.uri.fsPath);
  }
  return configured;
}

async function start(context) {
  if (!workspace.getConfiguration('rava').get('server.enable', true)) {
    return;
  }
  const command = serverPath();
  const serverOptions = {
    run: { command, transport: TransportKind.stdio },
    debug: { command, transport: TransportKind.stdio },
  };
  const clientOptions = {
    documentSelector: [{ scheme: 'file', language: 'rava' }],
    synchronize: { fileEvents: workspace.createFileSystemWatcher('**/*.rava') },
  };

  client = new LanguageClient('rava', 'Rava', serverOptions, clientOptions);
  try {
    await client.start();
    context.subscriptions.push(client);
  } catch (e) {
    window.showWarningMessage(
      `Rava : impossible de lancer « ${command} ». La coloration reste active. ` +
        'Installez le serveur avec `cargo install --path crates/rava-lsp`, ' +
        'ou indiquez son chemin dans le réglage `rava.server.path`.'
    );
  }
}

/** Ouvre le Rust produit par `ravac emit`, à côté du source. */
function showGeneratedRust() {
  const editor = window.activeTextEditor;
  if (!editor || !editor.document.fileName.endsWith('.rava')) {
    window.showInformationMessage('Rava : ouvrez d’abord un fichier .rava.');
    return;
  }
  const ravac = path.join(path.dirname(serverPath()), 'ravac');
  execFile(ravac, ['emit', editor.document.fileName], async (err, stdout, stderr) => {
    if (err) {
      window.showErrorMessage(`Rava : ${stderr || err.message}`);
      return;
    }
    const doc = await workspace.openTextDocument({ content: stdout, language: 'rust' });
    window.showTextDocument(doc, { preview: true, viewColumn: 2 });
  });
}

function activate(context) {
  context.subscriptions.push(
    commands.registerCommand('rava.showGeneratedRust', showGeneratedRust),
    commands.registerCommand('rava.restartServer', async () => {
      if (client) {
        await client.stop();
      }
      await start(context);
      window.showInformationMessage('Rava : serveur redémarré.');
    })
  );
  start(context);
}

function deactivate() {
  return client ? client.stop() : undefined;
}

module.exports = { activate, deactivate };
