import * as vscode from "vscode";
import { Sidecar } from "./sidecar";

export type SessionStatus = "pending" | "running" | "done" | "failed";

export interface TreeNode {
	id: string;
	parent_id: string | null;
	title: string;
	status: SessionStatus;
	model: string | null;
	summary: string | null;
	created_at: string;
}

/**
 * The session graph in the sidebar. The tree comes from the core already
 * built — this class arranges nodes, it does not decide what a session is or
 * what it inherits.
 */
export class SessionTreeProvider implements vscode.TreeDataProvider<TreeNode> {
	private changed = new vscode.EventEmitter<TreeNode | undefined>();
	readonly onDidChangeTreeData = this.changed.event;
	private nodes: TreeNode[] = [];

	constructor(private readonly sidecar: Sidecar) {}

	async refresh(): Promise<void> {
		try {
			const result = (await this.sidecar.request("session.tree")) as { tree: TreeNode[] };
			this.nodes = result.tree;
		} catch (error) {
			// An empty tree with an error message beats a stale one that looks
			// current.
			this.nodes = [];
			vscode.window.showErrorMessage(`Cymose: ${(error as Error).message}`);
		}
		this.changed.fire(undefined);
	}

	getChildren(element?: TreeNode): TreeNode[] {
		const parent = element?.id ?? null;
		return this.nodes.filter((n) => (n.parent_id ?? null) === parent);
	}

	getTreeItem(node: TreeNode): vscode.TreeItem {
		const hasChildren = this.nodes.some((n) => n.parent_id === node.id);
		const item = new vscode.TreeItem(
			node.title,
			hasChildren
				? vscode.TreeItemCollapsibleState.Expanded
				: vscode.TreeItemCollapsibleState.None,
		);
		item.id = node.id;
		item.description = node.model ?? undefined;
		// The summary is what a child would inherit, so showing it on hover is
		// the cheapest way to answer "what does this session know?".
		item.tooltip = new vscode.MarkdownString(
			node.summary ? `**${node.title}**\n\n${node.summary}` : `**${node.title}**\n\n_no summary yet_`,
		);
		item.iconPath = icon(node.status);
		item.contextValue = `session.${node.status}`;
		item.command = {
			command: "cymose.showSession",
			title: "Show session",
			arguments: [node],
		};
		return item;
	}
}

function icon(status: SessionStatus): vscode.ThemeIcon {
	switch (status) {
		case "done":
			return new vscode.ThemeIcon("pass", new vscode.ThemeColor("testing.iconPassed"));
		case "failed":
			return new vscode.ThemeIcon("error", new vscode.ThemeColor("testing.iconFailed"));
		case "running":
			return new vscode.ThemeIcon("sync~spin");
		default:
			return new vscode.ThemeIcon("circle-outline");
	}
}
