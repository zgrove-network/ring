/**
 * Recent Zcash blocks, fetched once for everyone instead of once per visitor.
 *
 * Reading the explorer straight from the browser made every visitor their own
 * client against a service that rate-limits by address, so whether the page
 * worked depended on who else shared your exit. Here one fetch serves the
 * whole edge.
 *
 * The cache is not here for speed. It is here so that the page survives the
 * explorer: when a fetch fails, the last answer that worked is served with its
 * age attached, rather than an error. A market drawn from twelve-minute-old
 * blocks is worth something and says so; a blank page is worth nothing.
 */

const SOURCE = "https://api.blockchair.com/zcash/blocks";
const FIELDS = "id,time,coinbase_data_hex,guessed_miner";

/** How long an answer is served without asking again. */
const FRESH_SECONDS = 30;

/** How long a stale answer is still better than no answer. */
const KEEP_SECONDS = 3600;

const MAX_LIMIT = 100;

/** Cache entries are keyed on a URL of our own, never the visitor's. */
const KEY = "https://ring.internal/blocks";

interface Ctx {
  readonly request: Request;
  waitUntil(promise: Promise<unknown>): void;
}

function age(response: Response): number {
  const at = Number(response.headers.get("x-fetched-at") ?? "0");
  return at === 0 ? Number.POSITIVE_INFINITY : Math.floor((Date.now() - at) / 1000);
}

function serve(body: string, fetchedAt: number, stale: boolean): Response {
  return new Response(body, {
    headers: {
      "content-type": "application/json; charset=utf-8",
      "access-control-allow-origin": "*",
      "cache-control": `public, max-age=${FRESH_SECONDS}`,
      "x-fetched-at": String(fetchedAt),
      // Said out loud, so the page can show it rather than pretend.
      "x-data-age-seconds": String(Math.max(0, Math.floor((Date.now() - fetchedAt) / 1000))),
      "x-data-stale": stale ? "true" : "false",
    },
  });
}

export const onRequest = async (context: Ctx): Promise<Response> => {
  const asked = Number(new URL(context.request.url).searchParams.get("limit") ?? MAX_LIMIT);
  const limit = Number.isFinite(asked) ? Math.min(Math.max(1, Math.trunc(asked)), MAX_LIMIT) : MAX_LIMIT;

  const cache = caches.default;
  const cacheKey = new Request(KEY, { method: "GET" });
  const held = await cache.match(cacheKey);

  if (held !== undefined && age(held) < FRESH_SECONDS) {
    const body = await held.text();
    return serve(body, Number(held.headers.get("x-fetched-at")), false);
  }

  try {
    const upstream = await fetch(`${SOURCE}?limit=${limit}&fields=${FIELDS}`, {
      headers: { accept: "application/json" },
      signal: AbortSignal.timeout(8000),
    });
    if (!upstream.ok) throw new Error(`explorer returned ${upstream.status}`);

    const body = await upstream.text();
    // Refuse to cache something that is not the answer, or the next hour is
    // spent serving it.
    const parsed = JSON.parse(body) as { data?: unknown };
    if (!Array.isArray(parsed.data)) throw new Error("explorer returned no blocks");

    const now = Date.now();
    const stored = new Response(body, {
      headers: {
        "content-type": "application/json",
        "cache-control": `public, max-age=${KEEP_SECONDS}`,
        "x-fetched-at": String(now),
      },
    });
    context.waitUntil(cache.put(cacheKey, stored.clone()));
    return serve(body, now, false);
  } catch {
    if (held !== undefined) {
      // The explorer is having a bad minute. The blocks it gave us last time
      // are still true, they are just older, and the page is told how old.
      const body = await held.text();
      return serve(body, Number(held.headers.get("x-fetched-at")), true);
    }
    return new Response(
      JSON.stringify({ error: "no blocks yet", detail: "the explorer did not answer and nothing is held" }),
      { status: 503, headers: { "content-type": "application/json", "access-control-allow-origin": "*" } },
    );
  }
};
