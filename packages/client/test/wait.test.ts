import "./_setup.ts";

import { afterEach, describe, expect, test } from "bun:test";

import { E2EClient } from "../src/client.ts";
import { WaitTimeoutError } from "../src/errors.ts";
import type { WaitCondition } from "../src/types.gen.ts";

interface MockServer {
  baseUrl: string;
  receivedBodies: Array<{ path: string; method: string; body: unknown }>;
  stop(): Promise<void>;
}

interface RouteHandlers {
  wait?: (body: unknown) => Response | Promise<Response>;
}

function startMock(handlers: RouteHandlers): MockServer {
  const receivedBodies: Array<{ path: string; method: string; body: unknown }> = [];

  const server = Bun.serve({
    port: 0,
    async fetch(req) {
      const url = new URL(req.url);
      let body: unknown = null;
      if (req.method !== "GET" && req.headers.get("content-type")?.includes("application/json")) {
        try {
          body = await req.json();
        } catch {
          body = null;
        }
      }
      receivedBodies.push({ path: url.pathname, method: req.method, body });
      if (url.pathname === "/test/wait" && handlers.wait) return handlers.wait(body);
      return new Response("not found", { status: 404 });
    },
  });

  return {
    baseUrl: `http://127.0.0.1:${server.port}`,
    receivedBodies,
    async stop() {
      await server.stop(true);
    },
  };
}

describe("E2EClient.wait", () => {
  let mock: MockServer;

  afterEach(async () => {
    await mock.stop();
  });

  test("returns WaitResult on 200", async () => {
    mock = startMock({
      wait: () =>
        new Response(JSON.stringify({ elapsed_ms: 42 }), {
          status: 200,
          headers: { "content-type": "application/json" },
        }),
    });
    const client = new E2EClient({ baseUrl: mock.baseUrl });
    const cond: WaitCondition = { kind: "selector_visible", selector: "#btn1" };
    const r = await client.wait(cond, { timeoutMs: 1000 });
    expect(r.elapsed_ms).toBe(42);
  });

  test("serialises condition with kind discriminator", async () => {
    mock = startMock({
      wait: () =>
        new Response(JSON.stringify({ elapsed_ms: 1 }), {
          status: 200,
          headers: { "content-type": "application/json" },
        }),
    });
    const client = new E2EClient({ baseUrl: mock.baseUrl });
    const cond: WaitCondition = {
      kind: "state_eq",
      selector: "#label1",
      property: "label",
      value: "hello",
    };
    await client.wait(cond, { timeoutMs: 500 });
    const last = mock.receivedBodies.at(-1);
    expect(last?.body).toEqual({
      condition: { kind: "state_eq", selector: "#label1", property: "label", value: "hello" },
      timeout_ms: 500,
    });
  });

  test("throws WaitTimeoutError on 408", async () => {
    mock = startMock({
      wait: () =>
        new Response(JSON.stringify({ error: "wait_timeout", timeout_ms: 250 }), {
          status: 408,
          headers: { "content-type": "application/json" },
        }),
    });
    const client = new E2EClient({ baseUrl: mock.baseUrl });
    const cond: WaitCondition = { kind: "selector_visible", selector: "#never" };
    await expect(client.wait(cond, { timeoutMs: 250 })).rejects.toBeInstanceOf(WaitTimeoutError);
  });

  test("uses 5s default timeout when not specified", async () => {
    mock = startMock({
      wait: () =>
        new Response(JSON.stringify({ elapsed_ms: 0 }), {
          status: 200,
          headers: { "content-type": "application/json" },
        }),
    });
    const client = new E2EClient({ baseUrl: mock.baseUrl });
    const cond: WaitCondition = { kind: "selector_visible", selector: "#btn1" };
    await client.wait(cond);
    const last = mock.receivedBodies.at(-1);
    expect((last?.body as { timeout_ms: number }).timeout_ms).toBe(5000);
  });
});
