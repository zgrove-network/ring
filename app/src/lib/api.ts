/**
 * The ledger's face, when there is one.
 *
 * Without VITE_RING_API this file is never used and the market stays what it
 * has always been: a simulation on real chain data. With it, balances and
 * positions are the book's, and the interface has to stop saying otherwise.
 */

const BASE = import.meta.env["VITE_RING_API"] ?? "";

export const connected = BASE !== "";

/** The token is the only way back to a balance. Kept where the browser keeps
 * things, and treated as missing whenever that throws — a private window, a
 * cleared site, a browser that says no. */
const KEY = "ring.token";

export function heldToken(): string | null {
  try {
    return window.localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

function hold(token: string): boolean {
  try {
    window.localStorage.setItem(KEY, token);
    return true;
  } catch {
    return false;
  }
}

export interface Standing {
  readonly index: number;
  readonly address: string;
  readonly available: number;
}

export interface Joined extends Standing {
  readonly token: string;
  /** False when the browser refused to keep it, which the caller must show. */
  readonly kept: boolean;
}

async function call<T>(path: string, init?: RequestInit): Promise<T> {
  const token = heldToken();
  const response = await fetch(`${BASE}${path}`, {
    ...init,
    headers: {
      ...(init?.body === undefined ? {} : { "content-type": "application/json" }),
      ...(token === null ? {} : { authorization: `Bearer ${token}` }),
      ...init?.headers,
    },
  });

  const body: unknown = await response.json().catch(() => null);
  if (!response.ok) {
    const said = (body as { error?: string } | null)?.error;
    throw new Error(said ?? `the ledger answered ${response.status}`);
  }
  return body as T;
}

export async function join(): Promise<Joined> {
  const answer = await call<Standing & { token: string }>("/v1/join", { method: "POST" });
  return { ...answer, available: 0, kept: hold(answer.token) };
}

export function standing(): Promise<Standing> {
  return call<Standing>("/v1/me");
}

export function bet(settlesOn: number, outcome: string, zatoshi: number): Promise<Standing> {
  return call<Standing>("/v1/bet", {
    method: "POST",
    body: JSON.stringify({ settles_on: settlesOn, outcome, zatoshi }),
  });
}

export function withdraw(zatoshi: number, to: string): Promise<Standing> {
  return call<Standing>("/v1/withdraw", {
    method: "POST",
    body: JSON.stringify({ zatoshi, to }),
  });
}
