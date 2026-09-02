import type { Context } from "@deepseek-ai/cordis";
export declare const name = "cymose";
export declare const inject: string[];
export interface Config {
    /** In-process graph name if you keep more than one. */
    namespace?: string;
}
export declare const Config: Config;
export declare function apply(ctx: Context, config: Config): void;
