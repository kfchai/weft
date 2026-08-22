import * as vscode from 'vscode';
import * as cp from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

// The whole design premise of Weft is that `weftc` already emits
// machine-actionable JSON diagnostics citing numbered spec rules [W41]. So this
// extension is not a language server reimplementing analysis — it is a thin
// adapter that runs the real checker over the current buffer and maps its JSON
// onto VS Code's diagnostic model. The compiler stays the single source of truth.

let diagnostics: vscode.DiagnosticCollection;
let output: vscode.OutputChannel;

// One reusable temp file per document, so on-type checking never touches the
// user's unsaved buffer on disk. Keyed by document URI.
const tempFiles = new Map<string, string>();
let tempDir: string | undefined;

// Debounce timers per document.
const timers = new Map<string, NodeJS.Timeout>();

// We only want to nag about a missing weftc once per session.
let warnedMissing = false;

export function activate(context: vscode.ExtensionContext): void {
  diagnostics = vscode.languages.createDiagnosticCollection('weft');
  output = vscode.window.createOutputChannel('Weft');
  context.subscriptions.push(diagnostics, output);

  // Check documents already open when the extension activates.
  for (const doc of vscode.workspace.textDocuments) {
    if (isWeft(doc)) {
      void checkDocument(doc);
    }
  }

  context.subscriptions.push(
    vscode.workspace.onDidOpenTextDocument((doc) => {
      if (isWeft(doc)) {
        void checkDocument(doc);
      }
    }),
    vscode.workspace.onDidChangeTextDocument((e) => {
      if (isWeft(e.document) && getConfig().checkOnType) {
        scheduleCheck(e.document);
      }
    }),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (isWeft(doc)) {
        void checkDocument(doc);
      }
    }),
    vscode.workspace.onDidCloseTextDocument((doc) => {
      if (isWeft(doc)) {
        diagnostics.delete(doc.uri);
        cleanupTemp(doc);
      }
    }),
    vscode.commands.registerCommand('weft.check', () => withActiveWeft(checkDocument)),
    vscode.commands.registerCommand('weft.run', () => withActiveWeft(runInTerminal.bind(null, 'run'))),
    vscode.commands.registerCommand('weft.test', () => withActiveWeft(runInTerminal.bind(null, 'test'))),
    vscode.commands.registerCommand('weft.repairContext', () =>
      withActiveWeft((doc) => captureToDocument(doc, ['repair-context'], 'markdown'))
    ),
    vscode.commands.registerCommand('weft.skeleton', () =>
      withActiveWeft((doc) => captureToDocument(doc, ['skeleton'], 'weft'))
    )
  );
}

export function deactivate(): void {
  for (const [, file] of tempFiles) {
    try {
      fs.rmSync(file, { force: true });
    } catch {
      /* best effort */
    }
  }
  if (tempDir) {
    try {
      fs.rmSync(tempDir, { recursive: true, force: true });
    } catch {
      /* best effort */
    }
  }
}

// ---------------------------------------------------------------------------
// Config + weftc resolution
// ---------------------------------------------------------------------------

interface Config {
  weftcPath: string;
  checkOnType: boolean;
  debounceMs: number;
  showHoles: boolean;
}

function getConfig(): Config {
  const c = vscode.workspace.getConfiguration('weft');
  return {
    weftcPath: c.get<string>('weftcPath', 'weftc'),
    checkOnType: c.get<boolean>('checkOnType', true),
    debounceMs: c.get<number>('debounceMs', 300),
    showHoles: c.get<boolean>('showHoles', true),
  };
}

// If the user left the default `weftc`, prefer a release build sitting in the
// workspace before falling back to PATH — this repo builds one at
// weftc/target/release/weftc(.exe). Otherwise honour whatever they configured.
function resolveWeftc(doc: vscode.TextDocument): string {
  const configured = getConfig().weftcPath;
  if (configured !== 'weftc') {
    return configured;
  }
  const folder = vscode.workspace.getWorkspaceFolder(doc.uri);
  const roots: string[] = [];
  if (folder) {
    roots.push(folder.uri.fsPath);
  }
  for (const f of vscode.workspace.workspaceFolders ?? []) {
    roots.push(f.uri.fsPath);
  }
  const exe = process.platform === 'win32' ? 'weftc.exe' : 'weftc';
  for (const root of roots) {
    const candidate = path.join(root, 'weftc', 'target', 'release', exe);
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  return 'weftc';
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

function isWeft(doc: vscode.TextDocument): boolean {
  return doc.languageId === 'weft';
}

function scheduleCheck(doc: vscode.TextDocument): void {
  const key = doc.uri.toString();
  const existing = timers.get(key);
  if (existing) {
    clearTimeout(existing);
  }
  timers.set(
    key,
    setTimeout(() => {
      timers.delete(key);
      void checkDocument(doc);
    }, getConfig().debounceMs)
  );
}

function tempPathFor(doc: vscode.TextDocument): string {
  const key = doc.uri.toString();
  let file = tempFiles.get(key);
  if (!file) {
    if (!tempDir) {
      tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'weft-vscode-'));
    }
    file = path.join(tempDir, `${tempFiles.size}.weft`);
    tempFiles.set(key, file);
  }
  return file;
}

async function checkDocument(doc: vscode.TextDocument): Promise<void> {
  const weftc = resolveWeftc(doc);
  const cfg = getConfig();

  // Write the live buffer to a temp file so unsaved edits are checked too;
  // weftc reads from a path, not stdin. [W3] every program is self-contained,
  // so the temp file's location does not affect the result.
  const tmp = tempPathFor(doc);
  try {
    fs.writeFileSync(tmp, doc.getText(), 'utf8');
  } catch (e) {
    output.appendLine(`weft: could not write temp file: ${String(e)}`);
    return;
  }

  const cwd = workspaceCwd(doc);
  cp.execFile(
    weftc,
    ['check', '--json', tmp],
    { cwd, maxBuffer: 8 * 1024 * 1024 },
    (err, stdout, stderr) => {
      if (err && (err as NodeJS.ErrnoException).code === 'ENOENT') {
        warnMissingWeftc(weftc);
        return;
      }
      diagnostics.set(doc.uri, parseDiagnostics(stdout, stderr, cfg));
    }
  );
}

// weftc prints one JSON object per line: errors on stderr as
// {file, ok:false, error:{rule,message,span:{line,col,endLine,endCol},...}},
// hole notes on stdout as {note:"hole", rule, name, type, line, col}, and a
// success/summary line we ignore. Every line is parsed independently so one
// malformed line cannot suppress the rest.
function parseDiagnostics(stdout: string, stderr: string, cfg: Config): vscode.Diagnostic[] {
  const out: vscode.Diagnostic[] = [];
  const lines = (stderr + '\n' + stdout).split(/\r?\n/);

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed.startsWith('{')) {
      continue;
    }
    let obj: any;
    try {
      obj = JSON.parse(trimmed);
    } catch {
      continue;
    }

    if (obj.error && obj.error.span) {
      out.push(errorDiagnostic(obj.error));
    } else if (obj.note === 'hole' && cfg.showHoles) {
      out.push(holeDiagnostic(obj));
    }
  }
  return out;
}

function errorDiagnostic(error: any): vscode.Diagnostic {
  const s = error.span;
  const range = new vscode.Range(
    Math.max(0, (s.line ?? 1) - 1),
    Math.max(0, (s.col ?? 1) - 1),
    Math.max(0, (s.endLine ?? s.line ?? 1) - 1),
    Math.max(0, (s.endCol ?? (s.col ?? 1) + 1) - 1)
  );

  const parts: string[] = [String(error.message ?? 'error')];
  if (error.expected) {
    parts.push(`expected: ${error.expected}`);
  }
  if (error.actual) {
    parts.push(`actual: ${error.actual}`);
  }
  if (error.hint) {
    parts.push(`hint: ${error.hint}`);
  }

  const diag = new vscode.Diagnostic(range, parts.join('\n'), vscode.DiagnosticSeverity.Error);
  diag.source = 'weftc';
  if (error.rule) {
    // The rule id is the ground truth for what went wrong [W40]; link it to the spec.
    diag.code = {
      value: error.rule,
      target: vscode.Uri.parse(
        `https://github.com/kfchai/weft/blob/main/SPEC.md#${String(error.rule).toLowerCase()}`
      ),
    };
  }
  return diag;
}

function holeDiagnostic(note: any): vscode.Diagnostic {
  const line = Math.max(0, (note.line ?? 1) - 1);
  const col = Math.max(0, (note.col ?? 1) - 1);
  const width = String(note.name ?? '').length + 1; // include the leading `?`
  const range = new vscode.Range(line, col, line, col + width);
  const diag = new vscode.Diagnostic(
    range,
    `hole ?${note.name} has type ${note.type}`,
    vscode.DiagnosticSeverity.Information
  );
  diag.source = 'weftc';
  diag.code = 'W27';
  return diag;
}

function warnMissingWeftc(triedPath: string): void {
  if (warnedMissing) {
    return;
  }
  warnedMissing = true;
  void vscode.window
    .showErrorMessage(
      `Weft: could not run '${triedPath}'. Set "weft.weftcPath" to your weftc binary, ` +
        `or build it with 'cargo build --release' in weftc/.`,
      'Open Settings'
    )
    .then((choice) => {
      if (choice === 'Open Settings') {
        void vscode.commands.executeCommand('workbench.action.openSettings', 'weft.weftcPath');
      }
    });
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

function withActiveWeft(fn: (doc: vscode.TextDocument) => void | Promise<void>): void {
  const editor = vscode.window.activeTextEditor;
  if (!editor || !isWeft(editor.document)) {
    void vscode.window.showInformationMessage('Weft: no active .weft file.');
    return;
  }
  void fn(editor.document);
}

function workspaceCwd(doc: vscode.TextDocument): string {
  const folder = vscode.workspace.getWorkspaceFolder(doc.uri);
  return folder ? folder.uri.fsPath : path.dirname(doc.uri.fsPath);
}

// run/test go through a real terminal so read_line and interactive programs work.
async function runInTerminal(sub: 'run' | 'test', doc: vscode.TextDocument): Promise<void> {
  if (doc.isUntitled) {
    void vscode.window.showInformationMessage('Weft: save the file before running.');
    return;
  }
  await doc.save();
  const weftc = resolveWeftc(doc);
  const term =
    vscode.window.terminals.find((t) => t.name === 'Weft') ??
    vscode.window.createTerminal({ name: 'Weft', cwd: workspaceCwd(doc) });
  term.show(true);
  term.sendText(`${quote(weftc)} ${sub} ${quote(doc.uri.fsPath)}`);
}

// repair-context / skeleton produce text meant to be read or pasted; capture
// stdout and open it in a fresh editor tab rather than a terminal.
async function captureToDocument(
  doc: vscode.TextDocument,
  args: string[],
  language: string
): Promise<void> {
  if (doc.isUntitled) {
    void vscode.window.showInformationMessage('Weft: save the file first.');
    return;
  }
  await doc.save();
  const weftc = resolveWeftc(doc);

  cp.execFile(
    weftc,
    [...args, doc.uri.fsPath],
    { cwd: workspaceCwd(doc), maxBuffer: 8 * 1024 * 1024 },
    async (err, stdout, stderr) => {
      if (err && (err as NodeJS.ErrnoException).code === 'ENOENT') {
        warnMissingWeftc(weftc);
        return;
      }
      const body = stdout.trim() || stderr.trim() || '(no output)';
      const shown = await vscode.workspace.openTextDocument({ content: body, language });
      await vscode.window.showTextDocument(shown, { preview: true, viewColumn: vscode.ViewColumn.Beside });
    }
  );
}

function quote(p: string): string {
  return /\s/.test(p) ? `"${p}"` : p;
}

function cleanupTemp(doc: vscode.TextDocument): void {
  const key = doc.uri.toString();
  const file = tempFiles.get(key);
  if (file) {
    try {
      fs.rmSync(file, { force: true });
    } catch {
      /* best effort */
    }
    tempFiles.delete(key);
  }
}
