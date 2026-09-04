import type { GraphKind, GraphNode } from "./ir.js";
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
export declare function voiceFor(kind: GraphKind | string | undefined): Voice;
export declare function formatSummary(s: SessionSummary): string;
/**
 * Map a later node should see: ancestors, failed siblings, and anything
 * promoted onto this node. `kind` only changes the wording.
 */
export declare function inheritText(current: GraphNode, ancestors: GraphNode[], siblings?: GraphNode[], kind?: GraphKind): string;
/** Prompt-sized map for the focused node, or a short empty-state hint. */
export declare function mapForPrompt(focused: GraphNode | undefined, ancestors: GraphNode[], siblings: GraphNode[], kind?: GraphKind): string;
export declare function treeListing(nodes: GraphNode[], focusedId: string | null, kind?: GraphKind): string;
export declare function diffText(a: GraphNode, b: GraphNode): string;
export {};
