import { useEffect, useState } from "react";

// Returns a value that lags behind the input by `ms` milliseconds.
// Used to coalesce rapid keystrokes into a single fetch call from the
// search input. The cleanup cancels the pending update when the input
// changes again before the delay elapses, so only the final value in
// a burst reaches the consumer.
export function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return debounced;
}
