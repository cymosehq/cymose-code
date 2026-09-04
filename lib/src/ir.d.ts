/** Status of a node. `dead-end` is a failed path children must still see. */
export type NodeStatus = "active" | "done" | "failed" | "dead-end";
/** Which coding harness owns this node's live session. */
export type Host = "dsh" | "mcp";
/**
 * How to read the same canvas. Ops stay identical; the map text changes.
 * `session` — what was tried. `todo` — open work. `steps` — a procedure with forks.
 * `answer` — claims, evidence, and rejected lines of argument.
 */
export declare const GRAPH_KINDS: readonly ["session", "todo", "steps", "answer"];
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
export declare const GRAPH_VERSION: 1;
export type GraphFile = {
    version: typeof GRAPH_VERSION;
    kind: GraphKind;
    focused_id: string | null;
    nodes: GraphNode[];
};
export declare function parseKind(value: unknown): GraphKind;
export declare function emptyGraph(kind?: GraphKind): GraphFile;
