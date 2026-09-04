export interface Tickable {
  run(): number;
}

export class AlphaRunner implements Tickable {
  run(): number {
    return 1;
  }
}

export function run(): number {
  return new AlphaRunner().run();
}
