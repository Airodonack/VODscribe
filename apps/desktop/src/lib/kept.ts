// What's read of a VOD (its lines, what of it is transcribed, its marks,
// captions and chat), kept for the session once read, so going back to a VOD
// reads nothing again: the last KEEP VODs opened, each until what it keeps
// changes (api.ts drops it then, on the app's own events and on a change
// made here). A read that fails isn't kept.

/** How many VODs' reads are kept. */
export const KEEP = 8;

const vods = new Map<number, Map<string, Promise<unknown>>>();

/** VOD `vodId`'s reads, made the most recent (the oldest beyond KEEP let go). */
function readsOf(vodId: number): Map<string, Promise<unknown>> {
  let reads = vods.get(vodId);
  if (reads) vods.delete(vodId);
  else reads = new Map();
  vods.set(vodId, reads);
  while (vods.size > KEEP) vods.delete(vods.keys().next().value!);
  return reads;
}

function keep<T>(reads: Map<string, Promise<unknown>>, key: string, read: Promise<T>): Promise<T> {
  reads.set(key, read);
  read.catch(() => reads.get(key) === read && reads.delete(key));
  return read;
}

/** What `read` gives for VOD `vodId`, read once and kept under `key`. */
export function kept<T>(vodId: number, key: string, read: () => Promise<T>): Promise<T> {
  const reads = readsOf(vodId);
  return (reads.get(key) as Promise<T> | undefined) ?? keep(reads, key, read());
}

/** Each of `keys` for VOD `vodId`, those not kept read together:
 *  `read(missing)` gives the values of the keys at those indices, in order. */
export function keptEach<T>(vodId: number, keys: string[], read: (missing: number[]) => Promise<T[]>): Promise<T[]> {
  const reads = readsOf(vodId);
  const missing = keys.flatMap((key, i) => (reads.has(key) ? [] : [i]));
  if (missing.length) {
    const batch = read(missing);
    missing.forEach((i, j) =>
      keep(
        reads,
        keys[i],
        batch.then((values) => values[j]),
      ),
    );
  }
  return Promise.all(keys.map((key) => reads.get(key) as Promise<T>));
}

/** Lets go of what's kept for VOD `vodId` (null: every VOD's) whose key `which` picks (all, without). */
export function forget(vodId: number | null, which: (key: string) => boolean = () => true) {
  for (const [id, reads] of vods)
    if (vodId === null || id === vodId) for (const key of [...reads.keys()]) if (which(key)) reads.delete(key);
}
