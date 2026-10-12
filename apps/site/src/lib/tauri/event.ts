// Stands in for @tauri-apps/api/event: nothing happens in the background of the demo.
export type UnlistenFn = () => void;
export async function listen<T>(_event: string, _handler: (e: { payload: T }) => void): Promise<UnlistenFn> {
  return () => {};
}
