import { GraphStore } from "./graph.js";
export type ToolSpec = {
    name: string;
    description: string;
    inputSchema: {
        type: "object";
        properties: Record<string, {
            type: "string";
            description: string;
        }>;
        required?: string[];
    };
};
export declare const toolSpecs: ToolSpec[];
export declare const MAP_INSTRUCTIONS = "Cymose is a map, not a coding agent. Call cymose_kind to choose session, todo, steps, or answer. Call cymose_tree or cymose_inherit before repeating work. Persist with cymose_dump; restore with cymose_load. The graph lives in this process.";
export declare function runTool(store: GraphStore, name: string, rawArgs?: unknown): string;
