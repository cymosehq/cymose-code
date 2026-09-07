import { GraphStore } from "../src/graph.js";
import { MAP_INSTRUCTIONS, runTool, toolSpecs } from "../src/tools.js";
export const PROTOCOL = "2024-11-05";
export const SERVER_NAME = "cymose-code";
export const SERVER_VERSION = "0.1.4";
const SUPPORTED = new Set(["2024-11-05", "2025-03-26", "2025-06-18"]);
function textResult(text, isError = false) {
    return {
        content: [{ type: "text", text }],
        ...(isError ? { isError: true } : {}),
    };
}
export function createMcpHandler(store = new GraphStore("mcp")) {
    return function handle(message) {
        if (message.id === undefined)
            return null;
        const id = message.id;
        const method = message.method ?? "";
        try {
            if (method === "initialize") {
                const params = (message.params ?? {});
                const requested = typeof params.protocolVersion === "string" ? params.protocolVersion : PROTOCOL;
                return {
                    jsonrpc: "2.0",
                    id,
                    result: {
                        protocolVersion: SUPPORTED.has(requested) ? requested : PROTOCOL,
                        capabilities: { tools: { listChanged: false } },
                        serverInfo: { name: SERVER_NAME, version: SERVER_VERSION },
                        instructions: MAP_INSTRUCTIONS,
                    },
                };
            }
            if (method === "ping") {
                return { jsonrpc: "2.0", id, result: {} };
            }
            if (method === "tools/list") {
                return {
                    jsonrpc: "2.0",
                    id,
                    result: {
                        tools: toolSpecs.map((spec) => ({
                            name: spec.name,
                            description: spec.description,
                            inputSchema: spec.inputSchema,
                        })),
                    },
                };
            }
            if (method === "tools/call") {
                const params = (message.params ?? {});
                if (typeof params.name !== "string" || !params.name) {
                    return { jsonrpc: "2.0", id, error: { code: -32602, message: "tools/call requires name" } };
                }
                try {
                    const text = runTool(store, params.name, params.arguments ?? {});
                    return { jsonrpc: "2.0", id, result: textResult(text) };
                }
                catch (error) {
                    const text = error instanceof Error ? error.message : String(error);
                    return { jsonrpc: "2.0", id, result: textResult(text, true) };
                }
            }
            return { jsonrpc: "2.0", id, error: { code: -32601, message: `Unknown method ${method}` } };
        }
        catch (error) {
            const text = error instanceof Error ? error.message : String(error);
            return { jsonrpc: "2.0", id, error: { code: -32603, message: text } };
        }
    };
}
