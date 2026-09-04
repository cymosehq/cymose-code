import { randomUUID } from "node:crypto";
import { inheritText, mapForPrompt } from "./inherit.js";
import { emptyGraph, GRAPH_VERSION, parseKind, type GraphFile, type GraphKind, type GraphNode, type Host, type NodeStatus } from "./ir.js";

function now(): string {
	return new Date().toISOString();
}

export function parseGraph(data: unknown): GraphFile {
	if (!data || typeof data !== "object") throw new Error("Graph is not an object.");
	const g = data as GraphFile;
	if (g.version !== GRAPH_VERSION) throw new Error(`Unsupported graph version ${String((data as { version?: unknown }).version)}.`);
	if (!Array.isArray(g.nodes)) throw new Error("Graph is missing nodes.");
	return {
		version: GRAPH_VERSION,
		kind: parseKind(g.kind),
		focused_id: typeof g.focused_id === "string" || g.focused_id === null ? g.focused_id : null,
		nodes: g.nodes.map((n) => ({
			...n,
			promoted: n.promoted ?? null,
			summary: n.summary ?? null,
			host_session_id: n.host_session_id ?? null,
		})),
	};
}

/** In-process graph. Persistence is the host's file tools, not this module. */
export class GraphStore {
	readonly host: Host;
	private data: GraphFile = emptyGraph();

	constructor(host: Host = "dsh") {
		this.host = host;
	}

	load(): GraphFile {
		return this.data;
	}

	replace(graph: GraphFile): void {
		this.data = graph;
	}

	kind(): GraphKind {
		return this.data.kind;
	}

	setKind(kind: GraphKind): GraphKind {
		this.data.kind = kind;
		return kind;
	}

	dump(): string {
		return `${JSON.stringify(this.data, null, 2)}\n`;
	}

	restore(raw: string): GraphFile {
		this.data = parseGraph(JSON.parse(raw) as unknown);
		return this.data;
	}

	get(id: string): GraphNode | undefined {
		return this.data.nodes.find((n) => n.id === id);
	}

	focused(): GraphNode | undefined {
		if (!this.data.focused_id) return undefined;
		return this.data.nodes.find((n) => n.id === this.data.focused_id);
	}

	/** Ancestor chain, root first, not including `id`. */
	ancestors(id: string): GraphNode[] {
		const byId = new Map(this.data.nodes.map((n) => [n.id, n]));
		const chain: GraphNode[] = [];
		let cur = byId.get(id);
		const seen = new Set<string>();
		while (cur?.parent_id) {
			if (seen.has(cur.parent_id)) break;
			seen.add(cur.parent_id);
			const parent = byId.get(cur.parent_id);
			if (!parent) break;
			chain.unshift(parent);
			cur = parent;
		}
		return chain;
	}

	focus(id: string): GraphNode {
		const node = this.data.nodes.find((n) => n.id === id);
		if (!node) throw new Error(`No node ${id}.`);
		this.data.focused_id = id;
		return node;
	}

	branch(title: string, parentId: string | null): GraphNode {
		if (parentId && !this.data.nodes.some((n) => n.id === parentId)) {
			throw new Error(`No parent node ${parentId}.`);
		}
		const stamp = now();
		const node: GraphNode = {
			id: randomUUID(),
			parent_id: parentId,
			title: title.trim() || "untitled",
			summary: null,
			promoted: null,
			status: "active",
			host: this.host,
			host_session_id: null,
			created_at: stamp,
			updated_at: stamp,
		};
		this.data.nodes.push(node);
		this.data.focused_id = node.id;
		return node;
	}

	bindSession(id: string, hostSessionId: string): GraphNode {
		return this.patch(id, { host_session_id: hostSessionId });
	}

	setStatus(id: string, status: NodeStatus): GraphNode {
		return this.patch(id, { status });
	}

	setSummary(id: string, summary: string): GraphNode {
		return this.patch(id, { summary });
	}

	promote(childId: string, conclusion?: string): GraphNode {
		const child = this.get(childId);
		if (!child) throw new Error(`No node ${childId}.`);
		if (!child.parent_id) throw new Error("A root has nowhere to promote to.");
		const text = (conclusion ?? child.summary ?? "").trim();
		if (!text) throw new Error("Nothing to promote: write a summary first, or pass a conclusion.");
		return this.patch(child.parent_id, { promoted: text });
	}

	explore(parentId: string, titles: string[]): GraphNode[] {
		if (titles.length < 2) throw new Error("Explore needs at least two titles.");
		const created = titles.map((title) => this.branch(title, parentId));
		this.focus(parentId);
		return created;
	}

	siblings(id: string): GraphNode[] {
		const node = this.get(id);
		if (!node) return [];
		return this.data.nodes.filter((n) => n.parent_id === node.parent_id && n.id !== id);
	}

	/** Copy source summaries onto the target, verbatim, labeled by origin. */
	pick(targetId: string, sourceIds: string[]): GraphNode {
		const chunks: string[] = [];
		const target = this.get(targetId);
		if (!target) throw new Error(`No node ${targetId}.`);
		if (target.summary?.trim()) chunks.push(target.summary.trim());
		for (const sourceId of sourceIds) {
			const source = this.get(sourceId);
			if (!source) throw new Error(`No node ${sourceId}.`);
			const body = source.summary?.trim() || "(no summary)";
			chunks.push(`From "${source.title}" [${source.status}]:\n${body}`);
		}
		return this.setSummary(targetId, chunks.join("\n\n"));
	}

	context(id: string): string {
		const node = this.get(id);
		if (!node) throw new Error(`No node ${id}.`);
		return inheritText(node, this.ancestors(id), this.siblings(id), this.data.kind);
	}

	focusedContext(): string {
		const node = this.focused();
		if (!node) return mapForPrompt(undefined, [], [], this.data.kind);
		return inheritText(node, this.ancestors(node.id), this.siblings(node.id), this.data.kind);
	}

	private patch(id: string, fields: Partial<GraphNode>): GraphNode {
		const i = this.data.nodes.findIndex((n) => n.id === id);
		if (i < 0) throw new Error(`No node ${id}.`);
		const next = { ...this.data.nodes[i], ...fields, id: this.data.nodes[i].id, updated_at: now() };
		this.data.nodes[i] = next;
		return next;
	}
}
