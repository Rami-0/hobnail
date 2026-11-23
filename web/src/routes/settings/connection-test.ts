export type ConnectionTest = { url: string; pin: string; revision: number };

export function matchesConnectionTest(tested: ConnectionTest, current: ConnectionTest): boolean {
  return tested.revision === current.revision && tested.url === current.url && tested.pin === current.pin;
}
