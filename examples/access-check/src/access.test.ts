import { test } from "node:test";
import assert from "node:assert/strict";
import {
  attemptsWrite,
  denyReason,
  hasValidToken,
  isElevated,
  isPublicPath,
  withinQuota,
  type Request,
} from "./access.ts";

function request(overrides: Partial<Request> = {}): Request {
  return { method: "GET", path: "/data", token: "secret", role: "member", ...overrides };
}

test("health is public", () => {
  assert.equal(isPublicPath("/health"), true);
});

test("data is not public", () => {
  assert.equal(isPublicPath("/data"), false);
});

test("a matching token is valid", () => {
  assert.equal(hasValidToken(request(), "secret"), true);
});

test("a mismatched token is invalid", () => {
  assert.equal(hasValidToken(request({ token: "wrong" }), "secret"), false);
});

test("an admin is elevated", () => {
  assert.equal(isElevated(request({ role: "admin" })), true);
});

test("a member is not elevated", () => {
  assert.equal(isElevated(request({ role: "member" })), false);
});

test("POST attempts a write", () => {
  assert.equal(attemptsWrite(request({ method: "POST" })), true);
});

test("usage below the limit is within quota", () => {
  assert.equal(withinQuota(1, 10), true);
});

test("a public path is never denied", () => {
  assert.equal(denyReason(request({ path: "/health" }), "secret", 1, 10), null);
});

test("a bad token is unauthorized", () => {
  assert.equal(denyReason(request({ token: "wrong" }), "secret", 1, 10), "unauthorized");
});

test("a member writing is forbidden", () => {
  assert.equal(denyReason(request({ method: "POST" }), "secret", 1, 10), "forbidden");
});

test("an allowed read returns no reason", () => {
  assert.equal(denyReason(request(), "secret", 1, 10), null);
});
