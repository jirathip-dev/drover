// The tests' one way past `noUncheckedIndexedAccess`: a value the fixture must have, or the test fails naming it.
import assert from "node:assert/strict";

export function must<T>(x: T | null | undefined, what = "value"): T {
  assert.ok(x != null, `missing ${what}`);
  return x;
}
