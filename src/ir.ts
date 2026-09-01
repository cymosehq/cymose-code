/** Status of a coding node. `dead-end` is a failed approach children must see. */
export type NodeStatus = "active" | "done" | "failed" | "dead-end";

/** Which coding harness owns this node's live session. More hosts later. */
export type Host = "dsh";

/**
 * One short session in the graph. Children inherit summaries, not transcripts.
 * `host_session_id` is the DeepSeek Harness session this node maps to, when known.
 */
export type GraphNode = {
	id: string;
	parent_id: string | null;
	title: string;
	summary: string | null;
	status: NodeStatus;
	host: Host;
	host_session_id: string | null;
	web_workspace_id: string | null;
	created_at: string;
	updated_at: string;
};

export const GRAPH_VERSION = 1 as const;

export type GraphFile = {
	version: typeof GRAPH_VERSION;
	focused_id: string | null;
	nodes: GraphNode[];
};

export function emptyGraph(): GraphFile {
	return { version: GRAPH_VERSION, focused_id: null, nodes: [] };
}
