import "./_setup.ts";

import { afterEach, describe, expect, test } from "bun:test";
import type { ServerWebSocket } from "bun";

import { EventStreamError } from "../src/errors.ts";
import { openEventStream } from "../src/events.ts";
import type { EventEnvelope } from "../src/types.gen.ts";

interface MockWsServer {
  port: number;
  url: () => URL;
  /** Push a JSON envelope to every currently-connected client. */
  broadcast: (env: EventEnvelope) => void;
  /** Close the oldest live client socket, leaving the listener intact. */
  closeFirstClient: () => void;
  liveClients: () => number;
  stop: () => void;
}

function startMockWsServer(
  opts: {
    onOpen?: (ws: ServerWebSocket<unknown>) => void;
    onMessage?: (ws: ServerWebSocket<unknown>, msg: string) => void;
  } = {},
): MockWsServer {
  const sockets: ServerWebSocket<unknown>[] = [];
  const server = Bun.serve({
    port: 0,
    fetch(req, srv) {
      if (srv.upgrade(req)) return;
      return new Response("ws only", { status: 426 });
    },
    websocket: {
      open(ws) {
        sockets.push(ws);
        opts.onOpen?.(ws);
      },
      message(ws, msg) {
        opts.onMessage?.(ws, typeof msg === "string" ? msg : msg.toString());
      },
      close(ws) {
        const idx = sockets.indexOf(ws);
        if (idx >= 0) sockets.splice(idx, 1);
      },
    },
  });
  // Bun.serve(...).port is `number | undefined` for UDS support; we always
  // bind a TCP port (port: 0), so it is guaranteed non-null at runtime.
  const port = server.port!;
  return {
    port,
    url: () => new URL(`ws://127.0.0.1:${port}/test/events`),
    broadcast: (env) => {
      const json = JSON.stringify(env);
      for (const ws of sockets) ws.send(json);
    },
    closeFirstClient: () => {
      const first = sockets[0];
      first?.close();
    },
    liveClients: () => sockets.length,
    stop: () => server.stop(true),
  };
}

async function waitFor(predicate: () => boolean, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!predicate()) {
    if (Date.now() > deadline) {
      throw new Error(`waitFor timed out after ${timeoutMs}ms`);
    }
    await new Promise((r) => setTimeout(r, 10));
  }
}

let server: MockWsServer | null = null;

afterEach(() => {
  server?.stop();
  server = null;
});

describe("openEventStream", () => {
  test("yields an envelope received from the server", async () => {
    server = startMockWsServer();
    const stream = await openEventStream(
      { baseUrl: `http://127.0.0.1:${server.port}` },
      { urlBuilder: () => server!.url() },
    );

    // Wait until the server has registered the open before broadcasting.
    await waitFor(() => server!.liveClients() === 1, 1_000);
    server.broadcast({
      kind: "state_change",
      ts: "2024-01-02T03:04:05Z",
      data: { selector: "#label1" },
    });

    const { value, done } = await stream.next();
    expect(done).toBe(false);
    expect(value).toEqual({
      kind: "state_change",
      ts: "2024-01-02T03:04:05Z",
      data: { selector: "#label1" },
    });

    if (typeof stream.return === "function") {
      await stream.return();
    }
  });

  test("reconnects after a server-initiated close and resumes yielding", async () => {
    let openCount = 0;
    server = startMockWsServer({
      onOpen: (ws) => {
        openCount += 1;
        const env: EventEnvelope = {
          kind: "state_change",
          ts: String(openCount),
          data: { open: openCount },
        };
        ws.send(JSON.stringify(env));
      },
    });

    const stream = await openEventStream(
      { baseUrl: `http://127.0.0.1:${server.port}` },
      {
        urlBuilder: () => server!.url(),
        reconnect: { baseMs: 5, maxMs: 20, factor: 2, maxRetries: 5 },
      },
    );

    const first = await stream.next();
    expect(first.done).toBe(false);
    expect((first.value as EventEnvelope).ts).toBe("1");

    // Trigger reconnect: close the server-side socket but keep the listener
    // alive. The client should reopen on the same port.
    server.closeFirstClient();

    const second = await stream.next();
    expect(second.done).toBe(false);
    expect((second.value as EventEnvelope).ts).toBe("2");

    if (typeof stream.return === "function") {
      await stream.return();
    }
  });

  test("throws EventStreamError when the initial open fails", async () => {
    // Point at a closed loopback port. fetch() to a closed port resolves
    // with a connection refused on most OSes; WebSocket open will fail
    // synchronously with onerror/onclose.
    const url = new URL("ws://127.0.0.1:1/");
    await expect(
      openEventStream({ baseUrl: "http://127.0.0.1:1" }, { urlBuilder: () => url }),
    ).rejects.toBeInstanceOf(EventStreamError);
  });

  test("AbortSignal terminates the iterator with done=true", async () => {
    server = startMockWsServer();
    const ac = new AbortController();
    const stream = await openEventStream(
      { baseUrl: `http://127.0.0.1:${server.port}` },
      { urlBuilder: () => server!.url(), signal: ac.signal },
    );
    await waitFor(() => server!.liveClients() === 1, 1_000);

    const pending = stream.next();
    ac.abort();
    const result = await pending;
    expect(result.done).toBe(true);
  });
});
