/**
 * How to read the same canvas. Ops stay identical; the map text changes.
 * `session` — what was tried. `todo` — open work. `steps` — a procedure with forks.
 * `answer` — claims, evidence, and rejected lines of argument.
 */
export const GRAPH_KINDS = ["session", "todo", "steps", "answer"];
export const GRAPH_VERSION = 1;
export function parseKind(value) {
    if (value === undefined || value === null || value === "")
        return "session";
    if (typeof value === "string" && GRAPH_KINDS.includes(value)) {
        return value;
    }
    throw new Error(`Unknown graph kind ${String(value)}. Use ${GRAPH_KINDS.join(", ")}.`);
}
export function emptyGraph(kind = "session") {
    return { version: GRAPH_VERSION, kind, focused_id: null, nodes: [] };
}
