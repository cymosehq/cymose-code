import type { GraphNode } from "./ir.js";

/** Structured summary the harness model writes. Stored in process. */
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

function bodyFor(node: GraphNode): string {
	if (node.summary?.trim()) return node.summary.trim();
	if (node.status === "failed" || node.status === "dead-end") {
		return "(No summary stored — still a failed path. Do not retry it blindly.)";
	}
	return "(No summary yet.)";
}

/**
 * Map a later session should see: ancestors, failed siblings, and anything
 * promoted onto this node.
 */
export function inheritText(
	current: GraphNode,
	ancestors: GraphNode[],
	siblings: GraphNode[] = [],
): string {
	const lines = [
		`You are on Cymose node "${current.title}" (${current.id}).`,
		`Status: ${current.status}.`,
		"This is a map of what was already tried. Read it before choosing an approach.",
		"",
	];
	if (current.promoted?.trim()) {
		lines.push("## Folded in from a child");
		lines.push(current.promoted.trim());
		lines.push("");
	}
	if (ancestors.length === 0 && siblings.length === 0) {
		lines.push("No ancestor or sibling context yet. This is a root.");
		return lines.join("\n");
	}
	if (ancestors.length > 0) {
		lines.push("## Ancestors (root first)");
		for (const node of ancestors) {
			const tag = node.status === "dead-end" || node.status === "failed" ? " [do not repeat]" : "";
			lines.push(`### ${node.title} (${node.status})${tag}`);
			lines.push(bodyFor(node));
			if (node.promoted?.trim()) lines.push(`Folded in: ${node.promoted.trim()}`);
			lines.push("");
		}
	}
	const notable = siblings.filter((n) => n.id !== current.id);
	if (notable.length > 0) {
		lines.push("## Siblings (same parent)");
		for (const node of notable) {
			const tag = node.status === "dead-end" || node.status === "failed" ? " [do not repeat]" : "";
			lines.push(`### ${node.title} (${node.status})${tag}`);
			lines.push(bodyFor(node));
			lines.push("");
		}
	}
	return lines.join("\n").trimEnd();
}

/** Prompt-sized map for the focused node, or a short empty-state hint. */
export function mapForPrompt(
	focused: GraphNode | undefined,
	ancestors: GraphNode[],
	siblings: GraphNode[],
): string {
	if (!focused) {
		return [
			"Cymose is loaded. There is no focused node.",
			"Call cymose_branch to start a short session, or cymose_load to restore a dump.",
		].join(" ");
	}
	return inheritText(focused, ancestors, siblings);
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
			const bits = [n.summary ? "summary" : "", n.promoted ? "folded" : ""].filter(Boolean);
			const extra = bits.length ? ` (${bits.join(", ")})` : "";
			lines.push(`${"  ".repeat(depth)}- ${n.title} [${n.status}] ${n.id}${mark}${extra}`);
			walk(n.id, depth + 1);
		}
	};
	walk(null, 0);
	return lines.join("\n");
}

export function diffText(a: GraphNode, b: GraphNode): string {
	return [
		`# ${a.title} [${a.status}] ${a.id}`,
		a.summary?.trim() || "(no summary)",
		a.promoted?.trim() ? `Folded in: ${a.promoted.trim()}` : "",
		"",
		`# ${b.title} [${b.status}] ${b.id}`,
		b.summary?.trim() || "(no summary)",
		b.promoted?.trim() ? `Folded in: ${b.promoted.trim()}` : "",
	]
		.filter((line, i, all) => line !== "" || all[i - 1] !== "")
		.join("\n")
		.trim();
}
