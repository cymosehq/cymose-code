import type { GraphKind, GraphNode } from "./ir.js";
import { parseKind } from "./ir.js";

/** Structured summary the harness model writes. Stored in process. */
export type SessionSummary = {
	task: string;
	files_touched: string[];
	approach: string;
	outcome: string;
	key_decisions: string[];
	errors_encountered: string[];
};

type Voice = {
	header: string;
	youAre: (title: string, id: string) => string;
	mapHint: string;
	emptyRoot: string;
	ancestors: string;
	siblings: string;
	folded: string;
	failedTag: string;
	emptyPrompt: string;
	emptyTree: string;
	forked: string;
};

const VOICE: Record<GraphKind, Voice> = {
	session: {
		header: "Graph kind: session — what was already tried.",
		youAre: (title, id) => `You are on Cymose node "${title}" (${id}).`,
		mapHint: "This is a map of what was already tried. Read it before choosing an approach.",
		emptyRoot: "No ancestor or sibling context yet. This is a root.",
		ancestors: "## Ancestors (root first)",
		siblings: "## Siblings (same parent)",
		folded: "## Folded in from a child",
		failedTag: " [do not repeat]",
		emptyPrompt:
			"Cymose is loaded. There is no focused node. Call cymose_kind if this is a todo, steps, or answer graph, then cymose_branch — or cymose_load to restore a dump.",
		emptyTree: "The graph is empty. Call cymose_branch to start a node.",
		forked: "Forked approaches (parent stays focused):",
	},
	todo: {
		header: "Graph kind: todo — open work; blocked items stay visible.",
		youAre: (title, id) => `You are on todo "${title}" (${id}).`,
		mapHint: "This is the work graph. failed/dead-end items are blocked or dropped — children still see them.",
		emptyRoot: "No parent or sibling todos yet. This is a top-level item.",
		ancestors: "## Parent todos (root first)",
		siblings: "## Sibling todos",
		folded: "## Folded in from a child item",
		failedTag: " [blocked]",
		emptyPrompt:
			"Cymose todo graph is empty of focus. Call cymose_branch for an item, or cymose_load a dump.",
		emptyTree: "No todos yet. Call cymose_branch to add one.",
		forked: "Forked todo items (parent stays focused):",
	},
	steps: {
		header: "Graph kind: steps — a procedure that can fork; siblings are alternatives, not a forced sequence.",
		youAre: (title, id) => `You are on step "${title}" (${id}).`,
		mapHint: "This is the procedure map. Explore means alternative ways; failed/dead-end steps are paths not to replay blindly.",
		emptyRoot: "No earlier steps yet. This is the start of the procedure.",
		ancestors: "## Earlier steps (root first)",
		siblings: "## Alternative steps (same parent)",
		folded: "## Folded in from a child step",
		failedTag: " [abandoned]",
		emptyPrompt:
			"Cymose steps graph has no focused step. Call cymose_branch for the first step, or cymose_load a dump.",
		emptyTree: "No steps yet. Call cymose_branch to start the procedure.",
		forked: "Forked alternative steps (parent stays focused):",
	},
	answer: {
		header: "Graph kind: answer — claims and evidence; rejected lines stay on the map.",
		youAre: (title, id) => `You are on claim "${title}" (${id}).`,
		mapHint: "This is the answer graph. failed/dead-end siblings are rejected arguments. Promote folds a child into the parent claim.",
		emptyRoot: "No parent claim yet. This is a root of the answer.",
		ancestors: "## Parent claims (root first)",
		siblings: "## Rival claims (same parent)",
		folded: "## Folded in from a child claim",
		failedTag: " [rejected]",
		emptyPrompt:
			"Cymose answer graph has no focused claim. Call cymose_branch to start one, or cymose_load a dump.",
		emptyTree: "No claims yet. Call cymose_branch to start the answer.",
		forked: "Forked rival claims (parent stays focused):",
	},
};

export function voiceFor(kind: GraphKind | string | undefined): Voice {
	return VOICE[parseKind(kind)];
}

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

function bodyFor(node: GraphNode, kind: GraphKind): string {
	if (node.summary?.trim()) return node.summary.trim();
	if (node.status === "failed" || node.status === "dead-end") {
		if (kind === "todo") return "(No note stored — still blocked. Do not treat it as open work.)";
		if (kind === "steps") return "(No note stored — still an abandoned step. Do not replay it blindly.)";
		if (kind === "answer") return "(No note stored — still a rejected line. Do not reuse it as settled.)";
		return "(No summary stored — still a failed path. Do not retry it blindly.)";
	}
	return "(No summary yet.)";
}

function failed(node: GraphNode, voice: Voice): string {
	return node.status === "dead-end" || node.status === "failed" ? voice.failedTag : "";
}

/**
 * Map a later node should see: ancestors, failed siblings, and anything
 * promoted onto this node. `kind` only changes the wording.
 */
export function inheritText(
	current: GraphNode,
	ancestors: GraphNode[],
	siblings: GraphNode[] = [],
	kind: GraphKind = "session",
): string {
	const voice = voiceFor(kind);
	const lines = [voice.youAre(current.title, current.id), `Status: ${current.status}.`, voice.mapHint, ""];
	if (current.promoted?.trim()) {
		lines.push(voice.folded);
		lines.push(current.promoted.trim());
		lines.push("");
	}
	if (ancestors.length === 0 && siblings.length === 0) {
		lines.push(voice.emptyRoot);
		return lines.join("\n");
	}
	if (ancestors.length > 0) {
		lines.push(voice.ancestors);
		for (const node of ancestors) {
			lines.push(`### ${node.title} (${node.status})${failed(node, voice)}`);
			lines.push(bodyFor(node, kind));
			if (node.promoted?.trim()) lines.push(`Folded in: ${node.promoted.trim()}`);
			lines.push("");
		}
	}
	const notable = siblings.filter((n) => n.id !== current.id);
	if (notable.length > 0) {
		lines.push(voice.siblings);
		for (const node of notable) {
			lines.push(`### ${node.title} (${node.status})${failed(node, voice)}`);
			lines.push(bodyFor(node, kind));
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
	kind: GraphKind = "session",
): string {
	const voice = voiceFor(kind);
	if (!focused) return voice.emptyPrompt;
	return inheritText(focused, ancestors, siblings, kind);
}

export function treeListing(
	nodes: GraphNode[],
	focusedId: string | null,
	kind: GraphKind = "session",
): string {
	const voice = voiceFor(kind);
	if (nodes.length === 0) return `${voice.header}\n${voice.emptyTree}`;
	const byParent = new Map<string | null, GraphNode[]>();
	for (const n of nodes) {
		const key = n.parent_id;
		const list = byParent.get(key) ?? [];
		list.push(n);
		byParent.set(key, list);
	}
	const lines: string[] = [voice.header];
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
