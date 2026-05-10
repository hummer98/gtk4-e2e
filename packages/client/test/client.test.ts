import "./_setup.ts";

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { E2EClient } from "../src/client.ts";
import { HttpError, NotImplementedError } from "../src/errors.ts";
import type { Info } from "../src/types.gen.ts";

interface MockServer {
  baseUrl: string;
  receivedAuth: string[];
  receivedBodies: Array<{ path: string; method: string; body: unknown }>;
  stop(): Promise<void>;
}

function pngBytes(): Uint8Array {
  // Minimal PNG: 8-byte magic + IHDR + IDAT + IEND. Just enough for a magic byte
  // smoke check; the SDK does not parse the payload.
  return new Uint8Array([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae,
    0x42, 0x60, 0x82,
  ]);
}

interface RouteHandlers {
  info?: () => Response | Promise<Response>;
  tap?: (body: unknown) => Response | Promise<Response>;
  type?: (body: unknown) => Response | Promise<Response>;
  screenshot?: () => Response | Promise<Response>;
  swipe?: (body: unknown) => Response | Promise<Response>;
  elements?: (url: URL) => Response | Promise<Response>;
}

function startMock(handlers: RouteHandlers): MockServer {
  const receivedAuth: string[] = [];
  const receivedBodies: Array<{ path: string; method: string; body: unknown }> = [];

  const server = Bun.serve({
    port: 0,
    async fetch(req) {
      const url = new URL(req.url);
      receivedAuth.push(req.headers.get("authorization") ?? "");

      let body: unknown = null;
      if (req.method !== "GET" && req.headers.get("content-type")?.includes("application/json")) {
        try {
          body = await req.json();
        } catch {
          body = null;
        }
      }
      receivedBodies.push({ path: url.pathname, method: req.method, body });

      if (url.pathname === "/test/info" && handlers.info) return handlers.info();
      if (url.pathname === "/test/tap" && handlers.tap) return handlers.tap(body);
      if (url.pathname === "/test/type" && handlers.type) return handlers.type(body);
      if (url.pathname === "/test/screenshot" && handlers.screenshot) return handlers.screenshot();
      if (url.pathname === "/test/swipe" && handlers.swipe) return handlers.swipe(body);
      if (url.pathname === "/test/elements" && handlers.elements)
        return handlers.elements(url);
      return new Response("not found", { status: 404 });
    },
  });

  return {
    baseUrl: `http://127.0.0.1:${server.port}`,
    receivedAuth,
    receivedBodies,
    async stop() {
      await server.stop(true);
    },
  };
}

const sampleInfo: Info = {
  instance_id: "abc",
  pid: 4242,
  port: 19042,
  app_name: "gtk4-e2e-app",
  app_version: "0.1.0",
  capabilities: ["info"],
};

describe("E2EClient.getInfo", () => {
  let mock: MockServer;

  afterEach(async () => {
    await mock.stop();
  });

  test("parses /test/info JSON", async () => {
    mock = startMock({
      info: () => Response.json(sampleInfo),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    const got = await client.getInfo();
    expect(got).toEqual(sampleInfo);
  });

  test("sends Authorization: Bearer <token> when token is set", async () => {
    mock = startMock({
      info: () => Response.json(sampleInfo),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl, token: "secret" });
    await client.getInfo();
    expect(mock.receivedAuth.at(-1)).toBe("Bearer secret");
  });

  test("omits Authorization when token is unset", async () => {
    mock = startMock({
      info: () => Response.json(sampleInfo),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await client.getInfo();
    expect(mock.receivedAuth.at(-1)).toBe("");
  });
});

describe("E2EClient.tap", () => {
  let mock: MockServer;

  afterEach(async () => {
    await mock.stop();
  });

  test("sends {selector} for string target", async () => {
    mock = startMock({
      tap: () => new Response(null, { status: 204 }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await client.tap("#submit");

    const last = mock.receivedBodies.at(-1);
    expect(last?.method).toBe("POST");
    expect(last?.path).toBe("/test/tap");
    expect(last?.body).toEqual({ selector: "#submit" });
  });

  test("sends {xy:{x,y}} for coordinate target", async () => {
    mock = startMock({
      tap: () => new Response(null, { status: 204 }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await client.tap({ x: 1, y: 2 });

    expect(mock.receivedBodies.at(-1)?.body).toEqual({ xy: { x: 1, y: 2 } });
  });

  test("throws NotImplementedError on 501", async () => {
    mock = startMock({
      tap: () =>
        new Response(JSON.stringify({ error: "not_implemented", capability: "tap" }), {
          status: 501,
          headers: { "content-type": "application/json" },
        }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await expect(client.tap("#x")).rejects.toMatchObject({
      name: "NotImplementedError",
      capability: "tap",
      status: 501,
    });
  });

  // Plan Review M2: server returns 404 only for domain not-found (e.g.
  // selector_not_found). 501 is the dedicated capability-missing channel,
  // so 404 should surface as `HttpError` with the body intact for callers
  // to inspect.
  test("throws HttpError with selector_not_found body on 404", async () => {
    mock = startMock({
      tap: () =>
        new Response(JSON.stringify({ error: "selector_not_found", selector: "#x" }), {
          status: 404,
          headers: { "content-type": "application/json" },
        }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    let thrown: unknown;
    try {
      await client.tap("#x");
    } catch (err) {
      thrown = err;
    }
    expect(thrown).toBeInstanceOf(HttpError);
    expect((thrown as HttpError).status).toBe(404);
    expect((thrown as HttpError).body).toMatchObject({
      error: "selector_not_found",
      selector: "#x",
    });
  });

  test("throws HttpError on 500", async () => {
    mock = startMock({
      tap: () => new Response("boom", { status: 500 }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    let thrown: unknown;
    try {
      await client.tap("#x");
    } catch (err) {
      thrown = err;
    }
    expect(thrown).toBeInstanceOf(HttpError);
    expect((thrown as HttpError).status).toBe(500);
  });
});

describe("E2EClient.type", () => {
  let mock: MockServer;

  afterEach(async () => {
    await mock.stop();
  });

  test("sends {selector,text} as POST /test/type body", async () => {
    mock = startMock({
      type: () => new Response(null, { status: 200 }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await client.type("#input1", "hello");

    const last = mock.receivedBodies.at(-1);
    expect(last?.method).toBe("POST");
    expect(last?.path).toBe("/test/type");
    expect(last?.body).toEqual({ selector: "#input1", text: "hello" });
  });

  test("throws HttpError on 404 selector_not_found", async () => {
    mock = startMock({
      type: () =>
        new Response(JSON.stringify({ error: "selector_not_found", selector: "#x" }), {
          status: 404,
          headers: { "content-type": "application/json" },
        }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    let thrown: unknown;
    try {
      await client.type("#x", "foo");
    } catch (err) {
      thrown = err;
    }
    expect(thrown).toBeInstanceOf(HttpError);
    expect((thrown as HttpError).status).toBe(404);
  });

  test("throws NotImplementedError on 501", async () => {
    mock = startMock({
      type: () =>
        new Response(JSON.stringify({ error: "not_implemented", capability: "type" }), {
          status: 501,
          headers: { "content-type": "application/json" },
        }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await expect(client.type("#x", "y")).rejects.toMatchObject({
      name: "NotImplementedError",
      capability: "type",
      status: 501,
    });
  });
});

describe("E2EClient.swipe", () => {
  let mock: MockServer;

  afterEach(async () => {
    await mock.stop();
  });

  test("sends from / to / duration_ms POST body", async () => {
    mock = startMock({
      swipe: () => new Response(null, { status: 200 }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await client.swipe({ x: 1, y: 2 }, { x: 3, y: 4 }, 200);

    const last = mock.receivedBodies.at(-1);
    expect(last?.method).toBe("POST");
    expect(last?.path).toBe("/test/swipe");
    expect(last?.body).toEqual({
      from: { x: 1, y: 2 },
      to: { x: 3, y: 4 },
      duration_ms: 200,
    });
  });

  test("throws HttpError on 404 no_scrollable_at_point", async () => {
    mock = startMock({
      swipe: () =>
        new Response(
          JSON.stringify({ error: "no_scrollable_at_point", x: 1, y: 2 }),
          {
            status: 404,
            headers: { "content-type": "application/json" },
          },
        ),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    let thrown: unknown;
    try {
      await client.swipe({ x: 1, y: 2 }, { x: 1, y: 50 }, 100);
    } catch (err) {
      thrown = err;
    }
    expect(thrown).toBeInstanceOf(HttpError);
    expect((thrown as HttpError).status).toBe(404);
  });

  test("throws NotImplementedError on 501", async () => {
    mock = startMock({
      swipe: () =>
        new Response(
          JSON.stringify({ error: "not_implemented", capability: "swipe" }),
          {
            status: 501,
            headers: { "content-type": "application/json" },
          },
        ),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await expect(
      client.swipe({ x: 1, y: 2 }, { x: 3, y: 4 }, 100),
    ).rejects.toBeInstanceOf(NotImplementedError);
  });
});

describe("E2EClient.screenshot", () => {
  let mock: MockServer;
  let scratch: string;

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), "gtk4-e2e-shot-"));
  });

  afterEach(async () => {
    await mock.stop();
    rmSync(scratch, { recursive: true, force: true });
  });

  test("returns Uint8Array of PNG bytes when no path given", async () => {
    const png = pngBytes();
    mock = startMock({
      screenshot: () =>
        new Response(png, {
          status: 200,
          headers: { "content-type": "image/png" },
        }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    const got = await client.screenshot();
    expect(got).toBeInstanceOf(Uint8Array);
    expect(got[0]).toBe(0x89);
    expect(got[1]).toBe(0x50);
    expect(got[2]).toBe(0x4e);
    expect(got[3]).toBe(0x47);
  });

  test("writes file and returns path when path given", async () => {
    const png = pngBytes();
    mock = startMock({
      screenshot: () =>
        new Response(png, {
          status: 200,
          headers: { "content-type": "image/png" },
        }),
    });

    const out = join(scratch, "out.png");
    const client = new E2EClient({ baseUrl: mock.baseUrl });
    const returned = await client.screenshot(out);
    expect(returned).toBe(out);

    const written = await Bun.file(out).bytes();
    expect(written[0]).toBe(0x89);
    expect(written.byteLength).toBe(png.byteLength);
  });

  test("throws NotImplementedError on 501", async () => {
    mock = startMock({
      screenshot: () =>
        new Response(JSON.stringify({ error: "not_implemented", capability: "screenshot" }), {
          status: 501,
          headers: { "content-type": "application/json" },
        }),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    let thrown: unknown;
    try {
      await client.screenshot();
    } catch (err) {
      thrown = err;
    }
    expect(thrown).toBeInstanceOf(NotImplementedError);
    expect((thrown as NotImplementedError).capability).toBe("screenshot");
    expect((thrown as NotImplementedError).status).toBe(501);
  });
});

describe("E2EClient.elements", () => {
  let mock: MockServer;
  const empty = { roots: [], count: 0 };
  let lastUrl: URL | null = null;

  afterEach(async () => {
    await mock.stop();
    lastUrl = null;
  });

  test("issues GET /test/elements with no query when called without args", async () => {
    mock = startMock({
      elements: (url) => {
        lastUrl = url;
        return Response.json(empty);
      },
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    const got = await client.elements();
    expect(got).toEqual(empty);
    expect(lastUrl?.pathname).toBe("/test/elements");
    expect(lastUrl?.search).toBe("");
  });

  test("encodes selector and max_depth as query params", async () => {
    mock = startMock({
      elements: (url) => {
        lastUrl = url;
        return Response.json(empty);
      },
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await client.elements({ selector: "#input1", maxDepth: 2 });

    expect(lastUrl?.searchParams.get("selector")).toBe("#input1");
    expect(lastUrl?.searchParams.get("max_depth")).toBe("2");
  });

  test("encodes class selector with leading dot", async () => {
    mock = startMock({
      elements: (url) => {
        lastUrl = url;
        return Response.json(empty);
      },
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await client.elements({ selector: ".primary" });

    expect(lastUrl?.searchParams.get("selector")).toBe(".primary");
  });

  test("throws NotImplementedError on 501", async () => {
    mock = startMock({
      elements: () =>
        new Response(
          JSON.stringify({ error: "not_implemented", capability: "elements" }),
          {
            status: 501,
            headers: { "content-type": "application/json" },
          },
        ),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    await expect(client.elements()).rejects.toBeInstanceOf(NotImplementedError);
  });

  test("throws HttpError on 422", async () => {
    mock = startMock({
      elements: () =>
        new Response(
          JSON.stringify({ error: "invalid_selector", reason: "bad" }),
          {
            status: 422,
            headers: { "content-type": "application/json" },
          },
        ),
    });

    const client = new E2EClient({ baseUrl: mock.baseUrl });
    let thrown: unknown;
    try {
      await client.elements({ selector: "@bad" });
    } catch (err) {
      thrown = err;
    }
    expect(thrown).toBeInstanceOf(HttpError);
    expect((thrown as HttpError).status).toBe(422);
  });
});
