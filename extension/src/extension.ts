import * as path from "path";
import * as vscode from "vscode";
import { SessionTreeProvider, TreeNode } from "./sessionTree";
import { Sidecar } from "./sidecar";

let sidecar: Sidecar | undefined;

export async function activate(context: vscode.ExtensionContext): Promise<void> {
	const output = vscode.window.createOutputChannel("Cymose Code");
	context.subscriptions.push(output);

	const config = vscode.workspace.getConfiguration("cymose");
	const binary = resolveBinary(context, config.get<string>("corePath") ?? "");
	const storePath = config.get<string>("storePath") || undefined;

	sidecar = new Sidecar(binary, storePath, (line) => output.appendLine(line));

	const tree = new SessionTreeProvider(sidecar);
	context.subscriptions.push(vscode.window.registerTreeDataProvider("cymose.sessions", tree));

	const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Right, 100);
	status.text = "$(sync~spin) Cymose";
	status.show();
	context.subscriptions.push(status);

	try {
		await sidecar.start();
		const root = vscode.workspace.workspaceFolders?.[0];
		if (root) {
			await sidecar.request("workspace.open", { path: root.uri.fsPath });
			await tree.refresh();
		}
		const models = (await sidecar.request("model.list")) as { active: string | null };
		status.text = `$(chip) ${models.active ?? "Cymose"}`;
	} catch (error) {
		// A core that won't start is the difference between "no sessions yet"
		// and "nothing works" — say which.
		status.text = "$(error) Cymose";
		status.tooltip = (error as Error).message;
		output.appendLine(`failed to start core: ${(error as Error).message}`);
		vscode.window.showErrorMessage(`Cymose: ${(error as Error).message}`);
	}

	context.subscriptions.push(
		vscode.commands.registerCommand("cymose.refresh", () => tree.refresh()),

		vscode.commands.registerCommand("cymose.newSession", async () => {
			const title = await vscode.window.showInputBox({
				prompt: "What is this session for?",
				placeHolder: "fix the failing test in sliding_window",
			});
			if (!title || !sidecar) {
				return;
			}
			try {
				const result = (await sidecar.request("session.new", { title })) as {
					inherited: { title: string; outcome: string }[];
				};
				await tree.refresh();
				// Saying what was inherited is the whole product in one line —
				// silence here looks identical to a fresh, ignorant session.
				const inherited = result.inherited.length
					? `inherits ${result.inherited.map((i) => `${i.title} (${i.outcome})`).join(", ")}`
					: "root session, nothing inherited";
				output.appendLine(`session "${title}" — ${inherited}`);
				vscode.window.setStatusBarMessage(`Cymose: ${inherited}`, 5000);
			} catch (error) {
				vscode.window.showErrorMessage(`Cymose: ${(error as Error).message}`);
			}
		}),

		vscode.commands.registerCommand("cymose.showSession", async (node: TreeNode) => {
			if (!sidecar) {
				return;
			}
			try {
				const result = (await sidecar.request("session.resume", { id: node.id })) as {
					context: string;
				};
				output.show(true);
				output.appendLine(`\n=== ${node.title} (${node.status}) ===`);
				output.appendLine(result.context || "(nothing inherited)");
			} catch (error) {
				vscode.window.showErrorMessage(`Cymose: ${(error as Error).message}`);
			}
		}),

		vscode.commands.registerCommand("cymose.restartSidecar", async () => {
			sidecar?.stop();
			await sidecar?.start();
			await tree.refresh();
		}),
	);
}

export function deactivate(): void {
	// The core holds the session store open; leaving an orphan behind is worse
	// than killing it.
	sidecar?.stop();
	sidecar = undefined;
}

/**
 * Bundled binary first, then PATH. The setting wins over both, which is what
 * makes `cargo build` + reload a working edit cycle when the core is what you
 * are changing.
 */
function resolveBinary(context: vscode.ExtensionContext, configured: string): string {
	if (configured) {
		return configured;
	}
	const name = process.platform === "win32" ? "cymose.exe" : "cymose";
	const bundled = path.join(context.extensionPath, "bin", name);
	return require("fs").existsSync(bundled) ? bundled : name;
}
