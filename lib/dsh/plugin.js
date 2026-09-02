import { defineTool } from "@deepseek-ai/dsh-tools";
import { GraphStore } from "../src/graph.js";
import { runTool, toolSpecs } from "../src/tools.js";
export const name = "cymose";
export const inject = ["tools"];
export const Config = {
    namespace: "",
};
const graphs = new Map();
function store(config) {
    const key = config.namespace?.trim() || "default";
    const existing = graphs.get(key);
    if (existing)
        return existing;
    const created = new GraphStore("dsh");
    graphs.set(key, created);
    return created;
}
function textOut() {
    return {
        schema: { type: "string" },
        render: (_args, value) => [{ type: "text", text: value }],
    };
}
function dshParameters(spec) {
    const required = new Set(spec.inputSchema.required ?? []);
    const parameters = {};
    for (const [key, field] of Object.entries(spec.inputSchema.properties)) {
        parameters[key] = {
            type: "string",
            description: field.description,
            ...(required.has(key) ? { required: true } : {}),
        };
    }
    return parameters;
}
export function apply(ctx, config) {
    const prompts = ctx.get("systemPrompt");
    prompts?.section({
        name: "cymose:map",
        order: 45,
        text: () => store(config).focusedContext(),
    });
    for (const spec of toolSpecs) {
        ctx.tools.register(defineTool({
            name: spec.name,
            description: spec.description,
            parameters: dshParameters(spec),
            output: textOut(),
            async execute(args) {
                return runTool(store(config), spec.name, args);
            },
        }));
    }
}
