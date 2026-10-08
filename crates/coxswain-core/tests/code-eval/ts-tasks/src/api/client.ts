/**
 * The HTTP client for the Tasks API. Every request goes through `request`, which retries
 * failures with a growing wait.
 */

const BASE_URL = "/api/v1";
const MAX_RETRIES = 3;

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Fetch `path` as JSON; a network error or a 5xx answer is tried again up to MAX_RETRIES times, waiting 200 ms, 400 ms, 800 ms. */
export async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  let attempt = 0;
  for (;;) {
    try {
      const res = await fetch(`${BASE_URL}${path}`, { ...init, headers: { "Content-Type": "application/json", ...init.headers } });
      if (res.status >= 500 && attempt < MAX_RETRIES) throw new ApiError(res.status, res.statusText);
      if (!res.ok) throw new ApiError(res.status, await res.text());
      return (await res.json()) as T;
    } catch (e) {
      if (attempt >= MAX_RETRIES || (e instanceof ApiError && e.status < 500)) throw e;
      await sleep(200 * 2 ** attempt);
      attempt++;
    }
  }
}
