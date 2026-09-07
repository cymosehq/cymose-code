import { GraphStore } from "../src/graph.js";
export declare const PROTOCOL = "2024-11-05";
export declare const SERVER_NAME = "cymose-code";
export declare const SERVER_VERSION = "0.1.4";
type JsonRpcId = string | number | null;
export type JsonRpcRequest = {
    jsonrpc?: string;
    id?: JsonRpcId;
    method?: string;
    params?: unknown;
};
export type JsonRpcResponse = {
    jsonrpc: "2.0";
    id: JsonRpcId;
    result?: unknown;
    error?: {
        code: number;
        message: string;
    };
};
export declare function createMcpHandler(store?: GraphStore): (message: JsonRpcRequest) => JsonRpcResponse | null;
export type McpHandler = ReturnType<typeof createMcpHandler>;
export {};
