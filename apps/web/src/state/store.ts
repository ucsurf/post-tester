import { useCallback, useRef, useSyncExternalStore } from "react";

/** A minimal observable store; components subscribe to slices with `useStore`. */
export class Store<T> {
  private readonly listeners = new Set<() => void>();

  constructor(private state: T) {}

  get = (): T => this.state;

  set = (update: Partial<T> | ((state: T) => Partial<T>)): void => {
    const patch = typeof update === "function" ? update(this.state) : update;
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((listener) => listener());
  };

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
}

/** Re-renders only when the selected value changes (compared with `Object.is`, or `equal`). */
export function useStore<T, S>(store: Store<T>, selector: (state: T) => S, equal: (a: S, b: S) => boolean = Object.is): S {
  const last = useRef<{ state: T; selector: (state: T) => S; value: S } | null>(null);
  // Selectors are usually inline closures, so a new one is re-run once; `equal` keeps
  // the previous value (and its identity) when the result did not change.
  const getSnapshot = useCallback(() => {
    const state = store.get();
    const previous = last.current;
    if (previous && previous.state === state && previous.selector === selector) return previous.value;
    const computed = selector(state);
    const value = previous && equal(previous.value, computed) ? previous.value : computed;
    last.current = { state, selector, value };
    return value;
  }, [store, selector, equal]);
  return useSyncExternalStore(store.subscribe, getSnapshot, getSnapshot);
}

export function shallowEqualArrays<T>(a: readonly T[], b: readonly T[]): boolean {
  return a.length === b.length && a.every((v, i) => Object.is(v, b[i]));
}
