import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";
import { randomUUID } from "node:crypto";
import { emptyGraph, GRAPH_VERSION, type GraphFile, type GraphNode, type NodeStatus } from "./ir.js";

function now(): string {
	return new Date().toISOString();
}

function assertGraph(data: unknown): GraphFile {
	if (!data || typeof data !== "object") throw new Error("Graph file is not an object.");
	const g = data as GraphFile;
	if (g.version !== GRAPH_VERSION) throw new Error(`Unsupported graph version ${String((data as { version?: unknown }).version)}.`);
	if (!Array.isArray(g.nodes)) throw new Error("Graph file is missing nodes.");
	return {
		version: GRAPH_VERSION,
		focused_id: typeof g.focused_id === "string" || g.focused_id === null ? g.focused_id : null,
		nodes: g.nodes,
	};
}

export class GraphStore {
	constructor(private readonly path: string) {}

	load(): GraphFile {
		try {
			return assertGraph(JSON.parse(readFileSync(this.path, "utf8")));
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") return emptyGraph();
			throw error;
		}
	}

	save(graph: GraphFile): void {
		mkdirSync(dirname(this.path), { recursive: true });
		writeFileSync(this.path, `${JSON.stringify(graph, null, 2)}\n`, "utf8");
	}

	get(id: string): GraphNode | undefined {
		return this.load().nodes.find((n) => n.id === id);
	}

	focused(): GraphNode | undefined {
		const g = this.load();
		if (!g.focused_id) return undefined;
		return g.nodes.find((n) => n.id === g.focused_id);
	}

	/** Ancestor chain, root first, not including `id`. */
	ancestors(id: string): GraphNode[] {
		const g = this.load();
		const byId = new Map(g.nodes.map((n) => [n.id, n]));
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
		const g = this.load();
		const node = g.nodes.find((n) => n.id === id);
		if (!node) throw new Error(`No node ${id}.`);
		g.focused_id = id;
		this.save(g);
		return node;
	}

	branch(title: string, parentId: string | null): GraphNode {
		const g = this.load();
		if (parentId && !g.nodes.some((n) => n.id === parentId)) {
			throw new Error(`No parent node ${parentId}.`);
		}
		const stamp = now();
		const node: GraphNode = {
			id: randomUUID(),
			parent_id: parentId,
			title: title.trim() || "untitled",
			summary: null,
			status: "active",
			host: "dsh",
			host_session_id: null,
			web_workspace_id: null,
			created_at: stamp,
			updated_at: stamp,
		};
		g.nodes.push(node);
		g.focused_id = node.id;
		this.save(g);
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

	attachWeb(id: string, webWorkspaceId: string): GraphNode {
		return this.patch(id, { web_workspace_id: webWorkspaceId });
	}

	private patch(id: string, fields: Partial<GraphNode>): GraphNode {
		const g = this.load();
		const i = g.nodes.findIndex((n) => n.id === id);
		if (i < 0) throw new Error(`No node ${id}.`);
		const next = { ...g.nodes[i], ...fields, id: g.nodes[i].id, updated_at: now() };
		g.nodes[i] = next;
		this.save(g);
		return next;
	}
}
