export type Request = {
  method: string;
  path: string;
  token: string | null;
  role: string | null;
};

export function isPublicPath(path: string): boolean {
  if (path === "/health") return true;
  if (path === "/version") return true;
  return false;
}

export function hasValidToken(request: Request, expected: string | null): boolean {
  if (expected === null) return false;
  return request.token !== null && request.token === expected;
}

export function isElevated(request: Request): boolean {
  return request.role === "admin" || request.role === "owner";
}

export function attemptsWrite(request: Request): boolean {
  return request.method !== "GET";
}

export function withinQuota(used: number, limit: number): boolean {
  return used < limit;
}

export function denyReason(
  request: Request,
  expected: string | null,
  used: number,
  limit: number,
): string | null {
  if (isPublicPath(request.path)) return null;
  if (!hasValidToken(request, expected)) return "unauthorized";
  if (attemptsWrite(request) && !isElevated(request)) return "forbidden";
  if (!withinQuota(used, limit)) return "quota_exceeded";
  return null;
}
