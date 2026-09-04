import { type GraphFile, type GraphKind, type GraphNode, type Host, type NodeStatus } from "./ir.js";
export declare function parseGraph(data: unknown): GraphFile;
/** In-process graph. Persistence is the host's file tools, not this module. */
export declare class GraphStore {
    readonly host: Host;
    private data;
    constructor(host?: Host);
    load(): GraphFile;
    replace(graph: GraphFile): void;
    kind(): GraphKind;
    setKind(kind: GraphKind): GraphKind;
    dump(): string;
    restore(raw: string): GraphFile;
    get(id: string): GraphNode | undefined;
    focused(): GraphNode | undefined;
    /** Ancestor chain, root first, not including `id`. */
    ancestors(id: string): GraphNode[];
    focus(id: string): GraphNode;
    branch(title: string, parentId: string | null): GraphNode;
    bindSession(id: string, hostSessionId: string): GraphNode;
    setStatus(id: string, status: NodeStatus): GraphNode;
    setSummary(id: string, summary: string): GraphNode;
    promote(childId: string, conclusion?: string): GraphNode;
    explore(parentId: string, titles: string[]): GraphNode[];
    siblings(id: string): GraphNode[];
    /** Copy source summaries onto the target, verbatim, labeled by origin. */
    pick(targetId: string, sourceIds: string[]): GraphNode;
    context(id: string): string;
    focusedContext(): string;
    private patch;
}
