import type { GraphNode } from "./ir.js";

/** Structured summary from POST /v1/code/summarize. */
export type SessionSummary = {
	task: string;
	files_touched: string[];
	approach: string;
	outcome: string;
	key_decisions: string[];
	errors_encountered: string[];
};

export function formatSummary(s: SessionSummary): string {
	const lines = [
		`Task: ${s.task}`,
		`Outcome: ${s.outcome}`,
		s.approach ? `Approach: ${s.approach}` : "",
		s.files_touched.length ? `Files: ${s.files_touched.join(", ")}` : "",
		s.key_decisions.length ? `Keep: ${s.key_decisions.join("; ")}` : "",
		s.errors_encountered.length ? `Errors: ${s.errors_encountered.join("; ")}` : "",
	];
	return lines.filter(Boolean).join("\n");
}

/**
 * Text a later session should see. Failed and dead-end ancestors stay visible
 * so the model does not retry the same approach. Empty summaries are omitted
 * unless the node failed — then we still name it.
 */
export function inheritText(current: GraphNode, ancestors: GraphNode[]): string {
	const lines = [
		`You are on Cymose node "${current.title}" (${current.id}).`,
		`Status: ${current.status}.`,
		"This is a map of what was already tried. Do not treat it as a token-saving trick — read it.",
		"",
	];
	if (ancestors.length === 0) {
		lines.push("No ancestor summaries yet. This is a root.");
		return lines.join("\n");
	}
	lines.push("## Ancestors (root first)");
	for (const node of ancestors) {
		const tag = node.status === "dead-end" || node.status === "failed" ? " [do not repeat this approach]" : "";
		lines.push(`### ${node.title} (${node.status})${tag}`);
		if (node.summary?.trim()) lines.push(node.summary.trim());
		else if (node.status === "failed" || node.status === "dead-end") {
			lines.push("(No summary stored — still a failed path. Ask what broke before retrying it.)");
		} else {
			lines.push("(No summary yet.)");
		}
		lines.push("");
	}
	return lines.join("\n").trimEnd();
}

export function treeListing(nodes: GraphNode[], focusedId: string | null): string {
	if (nodes.length === 0) return "The graph is empty. Call cymose_branch to start a node.";
	const byParent = new Map<string | null, GraphNode[]>();
	for (const n of nodes) {
		const key = n.parent_id;
		const list = byParent.get(key) ?? [];
		list.push(n);
		byParent.set(key, list);
	}
	const lines: string[] = [];
	const walk = (parentId: string | null, depth: number) => {
		for (const n of byParent.get(parentId) ?? []) {
			const mark = n.id === focusedId ? " ← focused" : "";
			const sum = n.summary ? " has summary" : "";
			lines.push(`${"  ".repeat(depth)}- ${n.title} [${n.status}] ${n.id}${mark}${sum}`);
			walk(n.id, depth + 1);
		}
	};
	walk(null, 0);
	return lines.join("\n");
}
