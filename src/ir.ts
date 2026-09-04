/** Status of a node. `dead-end` is a failed path children must still see. */
export type NodeStatus = "active" | "done" | "failed" | "dead-end";

/** Which coding harness owns this node's live session. */
export type Host = "dsh" | "mcp";

/**
 * How to read the same canvas. Ops stay identical; the map text changes.
 * `session` — what was tried. `todo` — open work. `steps` — a procedure with forks.
 * `answer` — claims, evidence, and rejected lines of argument.
 */
export const GRAPH_KINDS = ["session", "todo", "steps", "answer"] as const;
export type GraphKind = (typeof GRAPH_KINDS)[number];

/**
 * One short node in the graph. Children inherit summaries, not transcripts.
 * `host_session_id` is the harness session this node maps to, when known.
 * `promoted` is a conclusion folded onto this node from a child (local canvas).
 */
export type GraphNode = {
	id: string;
	parent_id: string | null;
	title: string;
	summary: string | null;
	promoted: string | null;
	status: NodeStatus;
	host: Host;
	host_session_id: string | null;
	created_at: string;
	updated_at: string;
};

export const GRAPH_VERSION = 1 as const;

export type GraphFile = {
	version: typeof GRAPH_VERSION;
	kind: GraphKind;
	focused_id: string | null;
	nodes: GraphNode[];
};

export function parseKind(value: unknown): GraphKind {
	if (value === undefined || value === null || value === "") return "session";
	if (typeof value === "string" && (GRAPH_KINDS as readonly string[]).includes(value)) {
		return value as GraphKind;
	}
	throw new Error(`Unknown graph kind ${String(value)}. Use ${GRAPH_KINDS.join(", ")}.`);
}

export function emptyGraph(kind: GraphKind = "session"): GraphFile {
	return { version: GRAPH_VERSION, kind, focused_id: null, nodes: [] };
}
