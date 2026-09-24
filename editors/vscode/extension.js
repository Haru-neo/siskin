// Siskin 확장: 언어 서버(`siskin lsp`)를 띄우고, 실행 버튼을 붙입니다.
const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

let client = null;

function siskinPath() {
  return vscode.workspace.getConfiguration("siskin").get("path") || "siskin";
}

async function startClient(context) {
  const serverOptions = { command: siskinPath(), args: ["lsp"] };
  const clientOptions = {
    documentSelector: [{ scheme: "file", language: "siskin" }],
    outputChannelName: "Siskin",
  };
  client = new LanguageClient("siskin", "Siskin", serverOptions, clientOptions);
  try {
    await client.start();
  } catch (e) {
    client = null;
    vscode.window.showErrorMessage(
      "siskin 프로그램을 찾지 못했습니다. 설정에서 `siskin.path` 에 siskin 의 경로를 적어 주세요. (" + e.message + ")"
    );
  }
}

function runInTerminal(sub) {
  const ed = vscode.window.activeTextEditor;
  if (!ed || ed.document.languageId !== "siskin") {
    vscode.window.showWarningMessage("Siskin 파일(.skn)을 열고 눌러 주세요.");
    return;
  }
  ed.document.save().then(() => {
    let term = vscode.window.terminals.find((t) => t.name === "Siskin");
    if (!term) term = vscode.window.createTerminal("Siskin");
    term.show(true);
    const q = (s) => (process.platform === "win32" ? `"${s}"` : `'${s.replace(/'/g, "'\\''")}'`);
    term.sendText(`${q(siskinPath())} ${sub} ${q(ed.document.fileName)}`);
  });
}

async function activate(context) {
  context.subscriptions.push(
    vscode.commands.registerCommand("siskin.run", () => runInTerminal("run")),
    vscode.commands.registerCommand("siskin.build", () => runInTerminal("build")),
    vscode.commands.registerCommand("siskin.debug", () => runInTerminal("debug")),
    vscode.commands.registerCommand("siskin.restart", async () => {
      if (client) await client.stop();
      await startClient(context);
    })
  );
  await startClient(context);
}

async function deactivate() {
  if (client) await client.stop();
}

module.exports = { activate, deactivate };
