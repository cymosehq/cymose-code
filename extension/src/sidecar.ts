import { ChildProcessWithoutNullStreams, spawn } from "child_process";
import * as readline from "readline";

/** Bumped in lockstep with PROTOCOL_VERSION in cymose-core. */
export const PROTOCOL_VERSION = 1;

type Pending = {
	resolve: (value: unknown) => void;
	reject: (reason: Error) => void;
};

export type Notification = { method: string; params: unknown };

/**
 * Client for `cymose sidecar` — one child process per window, JSON-RPC over
 * stdio, newline delimited.
 *
 * Everything the extension knows comes through here. That is the point: the
 * session graph, the router and the agent loop exist once, in Rust, so this
 * client and the terminal cannot drift apart.
 */
export class Sidecar {
	private child?: ChildProcessWithoutNullStreams;
	private pending = new Map<number, Pending>();
	private nextId = 1;
	private listeners: ((n: Notification) => void)[] = [];

	constructor(
		private readonly binary: string,
		private readonly storePath: string | undefined,
		private readonly onLog: (message: string) => void,
	) {}

	async start(): Promise<void> {
		const args = this.storePath ? ["--store", this.storePath, "sidecar"] : ["sidecar"];
		const child = spawn(this.binary, args, { stdio: "pipe" });
		this.child = child;

		// stderr is logs, never protocol — see docs/sidecar-protocol.md.
		child.stderr.setEncoding("utf8");
		child.stderr.on("data", (chunk: string) => this.onLog(chunk.trimEnd()));

		readline.createInterface({ input: child.stdout }).on("line", (line) => this.receive(line));

		child.on("exit", (code) => {
			// Anything still waiting will never be answered; failing it beats a
			// command that hangs with no explanation.
			for (const [, pending] of this.pending) {
				pending.reject(new Error(`Cymose core exited (code ${code ?? "unknown"})`));
			}
			this.pending.clear();
			this.child = undefined;
		});

		const hello = (await this.request("initialize", {
			client: "vscode",
			client_version: PROTOCOL_VERSION,
			protocol: PROTOCOL_VERSION,
		})) as { protocol: number; core_version: string };

		// Refuse a mismatch outright. Probing method by method would turn one
		// clear "update the extension" into a scatter of unrelated failures.
		if (hello.protocol !== PROTOCOL_VERSION) {
			this.stop();
			throw new Error(
				`Cymose core speaks protocol ${hello.protocol}, this extension speaks ${PROTOCOL_VERSION}. ` +
					`Update whichever is older (core ${hello.core_version}).`,
			);
		}
	}

	stop(): void {
		this.child?.stdin.end();
		this.child?.kill();
		this.child = undefined;
	}

	onNotification(listener: (n: Notification) => void): void {
		this.listeners.push(listener);
	}

	request(method: string, params: unknown = {}): Promise<unknown> {
		const child = this.child;
		if (!child) {
			return Promise.reject(new Error("Cymose core is not running"));
		}
		const id = this.nextId++;
		return new Promise((resolve, reject) => {
			this.pending.set(id, { resolve, reject });
			child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`);
		});
	}

	private receive(line: string): void {
		if (!line.trim()) {
			return;
		}
		let message: {
			id?: number;
			method?: string;
			params?: unknown;
			result?: unknown;
			error?: { code: number; message: string };
		};
		try {
			message = JSON.parse(line);
		} catch {
			this.onLog(`unparsable line from core: ${line.slice(0, 200)}`);
			return;
		}

		// No id: a notification. Unknown ones are ignored on purpose — the core
		// may add events without a protocol bump.
		if (message.id === undefined) {
			if (message.method) {
				for (const listener of this.listeners) {
					listener({ method: message.method, params: message.params });
				}
			}
			return;
		}

		const pending = this.pending.get(message.id);
		if (!pending) {
			return;
		}
		this.pending.delete(message.id);
		if (message.error) {
			pending.reject(new Error(message.error.message));
		} else {
			pending.resolve(message.result);
		}
	}
}
