#!/usr/bin/env node
import { stdin, stdout } from "node:process";
import { createMcpHandler } from "./protocol.js";
const handle = createMcpHandler();
let pending = "";
function write(message) {
    stdout.write(`${JSON.stringify(message)}\n`);
}
function consume(chunk) {
    pending += chunk;
    if (pending.startsWith("Content-Length:")) {
        const split = pending.indexOf("\r\n\r\n");
        if (split < 0)
            return;
        const marker = "Content-Length:";
        const header = pending.slice(0, split);
        const line = header.split(/\r?\n/).find((row) => row.toLowerCase().startsWith(marker.toLowerCase()));
        if (!line) {
            pending = "";
            return;
        }
        const size = Number(line.slice(line.indexOf(":") + 1).trim());
        const bodyStart = split + 4;
        if (pending.length < bodyStart + size)
            return;
        const body = pending.slice(bodyStart, bodyStart + size);
        pending = pending.slice(bodyStart + size);
        dispatch(body);
        if (pending)
            consume("");
        return;
    }
    for (;;) {
        const newline = pending.indexOf("\n");
        if (newline < 0)
            return;
        const line = pending.slice(0, newline).replace(/\r$/, "");
        pending = pending.slice(newline + 1);
        if (line.trim())
            dispatch(line);
    }
}
function dispatch(raw) {
    let parsed;
    try {
        parsed = JSON.parse(raw);
    }
    catch {
        write({ jsonrpc: "2.0", id: null, error: { code: -32700, message: "Parse error" } });
        return;
    }
    const reply = handle(parsed);
    if (reply)
        write(reply);
}
stdin.setEncoding("utf8");
stdin.on("data", (chunk) => consume(chunk));
stdin.on("end", () => {
    if (pending.trim())
        dispatch(pending);
});
