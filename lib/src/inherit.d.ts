import type { GraphNode } from "./ir.js";
/** Structured summary the harness model writes. Stored in process. */
export type SessionSummary = {
    task: string;
    files_touched: string[];
    approach: string;
    outcome: string;
    key_decisions: string[];
    errors_encountered: string[];
};
export declare function formatSummary(s: SessionSummary): string;
/**
 * Map a later session should see: ancestors, failed siblings, and anything
 * promoted onto this node.
 */
export declare function inheritText(current: GraphNode, ancestors: GraphNode[], siblings?: GraphNode[]): string;
/** Prompt-sized map for the focused node, or a short empty-state hint. */
export declare function mapForPrompt(focused: GraphNode | undefined, ancestors: GraphNode[], siblings: GraphNode[]): string;
export declare function treeListing(nodes: GraphNode[], focusedId: string | null): string;
export declare function diffText(a: GraphNode, b: GraphNode): string;
