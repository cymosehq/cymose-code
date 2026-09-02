export const GRAPH_VERSION = 1;
export function emptyGraph() {
    return { version: GRAPH_VERSION, focused_id: null, nodes: [] };
}
