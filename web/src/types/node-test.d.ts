// The small part of Node's test runner and assert module the unit tests use.
// The project has no @types/node (no dependency without the lead, plan D3);
// these declarations let `tsc` check the *.test.ts files. Node runs them
// with type stripping, so nothing here exists at run time.
declare module "node:test" {
  type Fn = () => void | Promise<void>;
  export function test(name: string, fn: Fn): Promise<void>;
  export function describe(name: string, fn: Fn): Promise<void>;
  export function it(name: string, fn: Fn): Promise<void>;
}

declare module "node:assert/strict" {
  interface Assert {
    (value: unknown, message?: string): asserts value;
    ok(value: unknown, message?: string): asserts value;
    equal<T>(actual: unknown, expected: T, message?: string): asserts actual is T;
    notEqual(actual: unknown, expected: unknown, message?: string): void;
    deepEqual<T>(actual: unknown, expected: T, message?: string): asserts actual is T;
    match(value: string, re: RegExp, message?: string): void;
  }
  const assert: Assert;
  export default assert;
}
