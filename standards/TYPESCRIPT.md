# TypeScript

> Additive to STANDARDS.md. Read that first. Everything here is TypeScript-specific.
>
> Target: TypeScript 5.x strict mode. React 19, Redux Toolkit, Vitest. Tauri 2 desktop app + web UI.
>
> **Key decisions:** TS 5.x strict, React 19, Redux Toolkit, Vitest + happy-dom, Vite, Biome, React Compiler, Tauri 2, branded types, RTK Query.

---

## Toolchain

- **Language:** TypeScript 5.x, `strict: true` — zero `any` in new code
- **UI framework:** React 19 (function components only)
- **State:** Redux Toolkit (`@reduxjs/toolkit` + `react-redux`)
- **Testing:** Vitest + React Testing Library
- **Bundler:** Vite
- **Linter:** ESLint + typescript-eslint
- **Formatter:** Biome (preferred) or Prettier
- **Build/validate:**
  ```bash
  tsc --noEmit
  vitest run
  eslint .
  ```
- **Recommended tsconfig:**
  ```jsonc
  {
      "compilerOptions": {
          "target": "ES2024",
          "lib": ["ES2024", "DOM", "DOM.Iterable"],
          "module": "ESNext",
          "moduleResolution": "bundler",
          "strict": true,
          "noUncheckedIndexedAccess": true,
          "erasableSyntaxOnly": true,
          "skipLibCheck": true
      }
  }
  ```
  `erasableSyntaxOnly` bans `enum`, `namespace`, and constructor parameter properties — aligns with the type-stripping direction (Node `--experimental-strip-types`, Deno native TS). Use `as const` objects instead of enums.

---

## Naming

| Element | Convention | Example |
|---------|-----------|---------|
| Files | `kebab-case.ts` / `kebab-case.tsx` | `session-store.ts`, `track-item.tsx` |
| Components | `PascalCase` | `TrackItem`, `PlayerScreen` |
| Functions / Variables | `camelCase` | `loadConfig`, `isPlaying` |
| Constants | `UPPER_SNAKE_CASE` | `MAX_RETRIES`, `DEFAULT_TIMEOUT` |
| Types / Interfaces | `PascalCase` | `SessionConfig`, `MediaProvider` |
| Hooks | `use` prefix, `camelCase` | `usePlayer`, `useAlbum` |

- No `I` prefix on interfaces — `SessionConfig` not `ISessionConfig`
- Component files: one component per file, filename matches component name in kebab-case
- Event handlers: `onVerb` for props, `handleVerb` for implementations

---

## Type System

### Strict Mode, Zero `any`

`any` is banned in new code. Use `unknown` for truly unknown types and narrow with type guards.

```typescript
// Wrong
function process(data: any) { ... }

// Right
function process(data: unknown) {
    if (typeof data === 'string') { ... }
}
```

### Discriminated Unions for State

```typescript
type PlayerState =
    | { status: 'loading' }
    | { status: 'playing'; track: Track; progress: number }
    | { status: 'error'; message: string };
```

### `as const` for Literal Types

```typescript
const ROUTES = {
    home: '/',
    player: '/player',
    settings: '/settings',
} as const;
```

### `satisfies` for Type Validation Without Widening

Use `satisfies` when you want type checking at definition without losing the narrower inferred type. Prefer over explicit annotation when the narrow type matters.

```typescript
// Type-checked AND preserves literal types
const ROUTES = {
    home: '/',
    player: '/player',
    settings: '/settings',
} as const satisfies Record<string, string>;
// typeof ROUTES.home is '/', not string

// Without satisfies — type widens to string
const ROUTES: Record<string, string> = { home: '/' };
// typeof ROUTES.home is string
```

Use `satisfies` when:
- Config objects where you want both validation and autocomplete on literal values
- Mapping objects that must conform to a shape but need preserved literals for type narrowing

### `const` Type Parameters

Use `const` type parameters when inference should preserve literal types from arguments.

```typescript
function createRoute<const T extends readonly string[]>(paths: T): T {
    return paths;
}
// Infers ['/', '/player'] not string[]
const routes = createRoute(['/', '/player']);
```

### `NoInfer<T>` for Inference Control

Prevents a type parameter position from influencing inference — forces inference from other call sites.

```typescript
function createFSM<S extends string>(
    initial: S,
    transitions: Record<S, NoInfer<S>[]>,
) { ... }

// 'idle' | 'playing' | 'paused' inferred from first arg only
createFSM('idle', {
    idle: ['playing'],
    playing: ['paused', 'idle'],
    paused: ['playing', 'idle'],
});
```

### `Promise.withResolvers<T>()`

ES2024. Replaces the deferred promise constructor pattern. Returns `{ promise, resolve, reject }` as a single object.

```typescript
// Before: boilerplate constructor
let resolve!: (value: T) => void;
const promise = new Promise<T>(r => { resolve = r; });

// After: one-liner
const { promise, resolve } = Promise.withResolvers<T>();
```

Use for: event-to-promise adapters, test harnesses, and any case where resolve/reject must be called outside the constructor callback.

### `using` for Resource Management

Explicit resource management via `using` and `await using` for resources that need deterministic cleanup.

```typescript
function readConfig(path: string) {
    using handle = openFile(path); // Symbol.dispose called at block exit
    return parse(handle.read());
}

async function withConnection() {
    await using conn = await pool.acquire(); // Symbol.asyncDispose called
    return conn.query('SELECT ...');
}
```

Use `using` for: file handles, database connections, locks, temporary resources.
Requires TypeScript 5.2+ and a polyfill or runtime support for `Symbol.dispose`.

### Inferred Type Predicates (TS 5.5+)

TypeScript now infers `x is T` return types automatically for simple narrowing functions. Delete explicit type predicates that only existed for `.filter()` workarounds.

```typescript
// Before 5.5: explicit predicate required
const valid = results.filter((r): r is Result => r !== undefined);

// After 5.5: inferred automatically
const valid = results.filter((r) => r !== undefined);
// Type: Result[] — narrowed correctly

// Works with discriminated unions
const clicks = actions.filter((a) => a.type === 'click');
// Inferred as { type: 'click'; x: number }[]
```

### Branded Types for Domain IDs

Domain IDs are branded types, not bare strings. Prevents swapping session IDs with track IDs.

```typescript
type Brand<T, B extends string> = T & { readonly __brand: B };

type TrackId = Brand<string, 'TrackId'>;
type SessionId = Brand<string, 'SessionId'>;

function createTrackId(id: string): TrackId {
    return id as TrackId;
}

// Compile error: SessionId is not assignable to TrackId
function loadTrack(id: TrackId) { ... }
loadTrack(sessionId); // Error
```

---

## Error Handling

- Typed error classes with context, never bare `throw new Error(...)`
- Every `catch` block must log, rethrow, or return a meaningful value
- `void` prefix for intentional fire-and-forget promises
- No floating promises — every `async` call must be awaited or explicitly voided
- Use discriminated union results for expected failures (API calls, parsing)

```typescript
type Result<T, E = AppError> =
    | { ok: true; value: T }
    | { ok: false; error: E };

// Expected failure: return Result
async function fetchTrack(id: TrackId): Promise<Result<Track>> {
    const response = await fetch(`/api/tracks/${id}`);
    if (!response.ok) {
        return { ok: false, error: { kind: 'not_found', id } };
    }
    return { ok: true, value: await response.json() };
}

// Unexpected failure: throw typed error
class TauriIpcError extends Error {
    constructor(
        public readonly command: string,
        public readonly cause: unknown,
    ) {
        super(`IPC command "${command}" failed`);
        this.name = 'TauriIpcError';
    }
}
```

---

## React 19

### Function Components Only

No class components. No `React.FC` — it has incorrect children inference and adds noise.

```typescript
// Standard component signature
function TrackItem({ track, onPlay }: TrackItemProps) {
    return <div>...</div>;
}
```

### Ref as Prop (No `forwardRef`)

React 19 passes `ref` as a regular prop. `forwardRef` is no longer needed.

```typescript
// React 19: ref is just a prop
function TextInput({ ref, label, ...props }: TextInputProps & { ref?: React.Ref<HTMLInputElement> }) {
    return (
        <label>
            {label}
            <input ref={ref} {...props} />
        </label>
    );
}

// Ref cleanup functions (React 19)
function VideoPlayer({ src }: { src: string }) {
    const videoRef = useRef<HTMLVideoElement>(null);

    return (
        <video
            ref={(node) => {
                // Setup
                node?.play();
                // Cleanup — returned function runs on unmount or ref change
                return () => node?.pause();
            }}
            src={src}
        />
    );
}
```

### Context as Provider

React 19 renders `<Context>` directly — no more `.Provider`.

```typescript
const ThemeContext = createContext<Theme>('dark');

// React 19
function App() {
    return (
        <ThemeContext value="light">
            <Player />
        </ThemeContext>
    );
}

// NOT: <ThemeContext.Provider value="light">
```

### `use()` Hook

`use()` reads context and resolves promises in render. Works with Suspense for data fetching.

```typescript
import { use, Suspense } from 'react';

function TrackDetails({ trackPromise }: { trackPromise: Promise<Track> }) {
    const track = use(trackPromise); // Suspends until resolved
    return <h1>{track.title}</h1>;
}

// Wrap with Suspense boundary
function TrackPage({ id }: { id: TrackId }) {
    const trackPromise = fetchTrack(id); // Start fetching outside render
    return (
        <Suspense fallback={<TrackSkeleton />}>
            <TrackDetails trackPromise={trackPromise} />
        </Suspense>
    );
}

// use() for conditional context
function OptionalTheme() {
    if (shouldUseTheme) {
        const theme = use(ThemeContext);
        return <div style={{ color: theme.primary }}>...</div>;
    }
    return <div>...</div>;
}
```

### Actions and Form Handling

React 19 Actions integrate with transitions for async form operations.

```typescript
import { useActionState, useOptimistic, useTransition } from 'react';

// useActionState — form action with pending state
function AddToPlaylist({ trackId }: { trackId: TrackId }) {
    const [state, submitAction, isPending] = useActionState(
        async (_prev: ActionState, formData: FormData) => {
            const playlistId = formData.get('playlist') as string;
            const result = await addTrack(playlistId, trackId);
            return result.ok ? { success: true } : { error: result.error };
        },
        { success: false },
    );

    return (
        <form action={submitAction}>
            <select name="playlist">...</select>
            <button disabled={isPending}>
                {isPending ? 'Adding...' : 'Add'}
            </button>
            {state.error && <p>{state.error}</p>}
        </form>
    );
}

// useOptimistic — instant UI feedback before server confirmation
function PlaylistItems({ items }: { items: PlaylistItem[] }) {
    const [optimisticItems, addOptimistic] = useOptimistic(
        items,
        (current, newItem: PlaylistItem) => [...current, newItem],
    );

    async function handleAdd(track: Track) {
        const optimistic = { ...track, pending: true };
        addOptimistic(optimistic);
        await addToPlaylist(track.id); // Reverts if this throws
    }

    return <ul>{optimisticItems.map(item => <li key={item.id}>...</li>)}</ul>;
}
```

### Document Metadata

React 19 hoists `<title>`, `<meta>`, and `<link>` to `<head>` automatically.

```typescript
function AlbumPage({ album }: { album: Album }) {
    return (
        <>
            <title>{album.name} — Harmonia</title>
            <meta name="description" content={album.description} />
            <AlbumView album={album} />
        </>
    );
}
```

### Hooks Rules

- `useEffect` — cleanup function required for subscriptions, timers, listeners
- Custom hooks extract reusable logic: `usePlayer()`, `useAlbum(id)`
- Dependency arrays must be exhaustive — lint enforces this
- Prefer `useTransition` for non-urgent state updates over raw `setState`

### `useFormStatus` for Child Components

Reads pending state of the nearest parent `<form>`. No prop drilling needed.

```typescript
import { useFormStatus } from 'react-dom';

function SubmitButton({ label }: { label: string }) {
    const { pending } = useFormStatus();
    return (
        <button type="submit" disabled={pending}>
            {pending ? 'Saving...' : label}
        </button>
    );
}
```

Must be rendered inside a `<form>`. Returns `{ pending, data, method, action }`.

### React Compiler

Stable since October 2025. Ships as a Babel plugin. Auto-memoizes components, values, and callbacks at build time — including conditional paths that manual memoization cannot cover.

**Vite setup:**

```typescript
// vite.config.ts
export default defineConfig({
    plugins: [
        react({
            babel: {
                plugins: ['babel-plugin-react-compiler'],
            },
        }),
    ],
});
```

**Directives for incremental control:**

```typescript
// Opt in (when using compilationMode: "annotation")
function ExpensiveList({ items }: Props) {
    "use memo";
    return items.map(i => <Item key={i.id} data={i} />);
}

// Opt out (any mode — escape hatch)
function LegacyWidget() {
    "use no memo";
    // compiler leaves this alone
}
```

**Manual memoization rules:**

- **With compiler:** Do not write `useMemo`, `useCallback`, `React.memo`. The compiler handles it. Manual memoization is dead code.
- **Without compiler:** Use `useMemo` for expensive computations only. Use `useCallback` only when passing to memoized children. Use `React.memo` for components that re-render frequently with same props.
- **Never:** Memoize cheap operations. The memoization overhead exceeds the savings.

---

## State Management (Redux Toolkit)

One slice per domain. Redux Toolkit handles immutable updates via Immer internally, `createAsyncThunk` for async operations, and `createSelector` for memoized derived state.

### Store setup

```typescript
import { configureStore } from '@reduxjs/toolkit';
import { playerReducer } from './slices/player-slice';
import { queueReducer } from './slices/queue-slice';

export const store = configureStore({
    reducer: {
        player: playerReducer,
        queue: queueReducer,
    },
});

export type RootState = ReturnType<typeof store.getState>;
export type AppDispatch = typeof store.dispatch;
```

### Typed hooks

Define once, import everywhere. Never use untyped `useDispatch` or `useSelector`.

```typescript
import { useDispatch, useSelector } from 'react-redux';
import type { RootState, AppDispatch } from './store';

export const useAppDispatch = useDispatch.withTypes<AppDispatch>();
export const useAppSelector = useSelector.withTypes<RootState>();
```

### Slices

```typescript
import { createSlice, PayloadAction } from '@reduxjs/toolkit';

interface PlayerState {
    track: Track | null;
    isPlaying: boolean;
    volume: number;
}

const initialState: PlayerState = {
    track: null,
    isPlaying: false,
    volume: 1.0,
};

const playerSlice = createSlice({
    name: 'player',
    initialState,
    reducers: {
        play: (state, action: PayloadAction<Track>) => {
            state.track = action.payload;
            state.isPlaying = true;
        },
        pause: (state) => {
            state.isPlaying = false;
        },
        setVolume: (state, action: PayloadAction<number>) => {
            state.volume = action.payload;
        },
    },
});

export const { play, pause, setVolume } = playerSlice.actions;
export const playerReducer = playerSlice.reducer;
```

Immer runs inside `createSlice` reducers — mutative syntax (`state.track = action.payload`) produces immutable updates. Don't use Immer outside slices.

### Selectors

Colocate selectors with slices. Use `createSelector` for derived state.

```typescript
// Simple selectors
export const selectTrack = (state: RootState) => state.player.track;
export const selectIsPlaying = (state: RootState) => state.player.isPlaying;

// Memoized derived state
import { createSelector } from '@reduxjs/toolkit';

export const selectQueueDuration = createSelector(
    (state: RootState) => state.queue.items,
    (items) => items.reduce((sum, t) => sum + t.durationMs, 0),
);
```

In components:

```typescript
function Player() {
    const track = useAppSelector(selectTrack);
    return <span>{track?.title}</span>;
}
```

### Async (createAsyncThunk)

```typescript
import { createAsyncThunk, createSlice } from '@reduxjs/toolkit';

export const fetchAlbums = createAsyncThunk(
    'library/fetchAlbums',
    async (_, { rejectWithValue }) => {
        try {
            return await invoke<Album[]>('list_albums');
        } catch (error) {
            return rejectWithValue(error);
        }
    },
);

const librarySlice = createSlice({
    name: 'library',
    initialState: {
        albums: [] as Album[],
        loading: false,
        error: null as string | null,
    },
    reducers: {},
    extraReducers: (builder) => {
        builder
            .addCase(fetchAlbums.pending, (state) => {
                state.loading = true;
                state.error = null;
            })
            .addCase(fetchAlbums.fulfilled, (state, action) => {
                state.albums = action.payload;
                state.loading = false;
            })
            .addCase(fetchAlbums.rejected, (state, action) => {
                state.error = action.payload as string;
                state.loading = false;
            });
    },
});
```

### RTK Query for server state

RTK Query handles caching, background refetch, and invalidation. Use for all server/backend data instead of manual loading state in slices.

```typescript
import { createApi } from '@reduxjs/toolkit/query/react';
import { invoke } from '@tauri-apps/api/core';

const tauriBaseQuery = async (
    { command, args }: { command: string; args?: Record<string, unknown> },
) => {
    try {
        const data = await invoke(command, args);
        return { data };
    } catch (error) {
        return { error };
    }
};

export const api = createApi({
    reducerPath: 'api',
    baseQuery: tauriBaseQuery,
    tagTypes: ['Album', 'Track'],
    endpoints: (builder) => ({
        listAlbums: builder.query<Album[], void>({
            query: () => ({ command: 'list_albums' }),
            providesTags: ['Album'],
        }),
        importLibrary: builder.mutation<ImportReport, string>({
            query: (path) => ({ command: 'import_library', args: { path } }),
            invalidatesTags: ['Album', 'Track'],
        }),
    }),
});

export const { useListAlbumsQuery, useImportLibraryMutation } = api;
```

Add `api.reducer` and `api.middleware` to the store:

```typescript
export const store = configureStore({
    reducer: {
        player: playerReducer,
        [api.reducerPath]: api.reducer,
    },
    middleware: (getDefault) => getDefault().concat(api.middleware),
});
```

### Persistence

```typescript
import { combineReducers } from '@reduxjs/toolkit';
import { persistReducer, persistStore, createMigrate } from 'redux-persist';
import storage from 'redux-persist/lib/storage';

const persistConfig = {
    key: 'settings',
    storage,
    version: 2,
    migrate: createMigrate({
        2: (state) => ({ ...state, volume: 80 }),
    }),
};

const settingsPersistedReducer = persistReducer(persistConfig, settingsSlice.reducer);
```

Persist only user preferences. Never persist server-cached data or transient UI state.

### Redux anti-patterns

- **Direct state mutation outside reducers** — Immer only runs inside `createSlice` reducers; external mutation creates bugs
- **Business logic in components** — thunks and slices handle logic; components dispatch and select
- **Overusing Redux** — modal open/closed, form field values, and per-component state belong in `useState`
- **Missing `createSelector` for derived state** — recomputes on every render without memoization
- **Non-serializable values in state** — no functions, class instances, or `Date` objects. Use ISO strings for timestamps.
- **Server state in Redux slices** — use RTK Query for backend data (caching, refetch, invalidation)
- **Untyped hooks** — always use the typed `useAppDispatch`/`useAppSelector`, never raw `useDispatch`/`useSelector`

---

## Tauri 2 IPC

### Commands (Frontend → Backend)

Tauri commands are the primary frontend → backend bridge. Type safety is critical.

```typescript
import { invoke } from '@tauri-apps/api/core';

// Typed wrapper around invoke
async function listAlbums(): Promise<Album[]> {
    return invoke<Album[]>('list_albums');
}

async function playTrack(trackId: string): Promise<void> {
    return invoke('play_track', { trackId });
}

// Error handling across the bridge
async function importLibrary(path: string): Promise<Result<ImportReport>> {
    try {
        const report = await invoke<ImportReport>('import_library', { path });
        return { ok: true, value: report };
    } catch (error) {
        return { ok: false, error: { kind: 'ipc', command: 'import_library', cause: error } };
    }
}
```

### Events (Backend → Frontend)

Events for push notifications from Rust to the frontend.

```typescript
import { listen, emit } from '@tauri-apps/api/event';

// Listen with cleanup
function usePlaybackEvents() {
    useEffect(() => {
        const unlisten = listen<PlaybackEvent>('playback-state', (event) => {
            store.dispatch(handlePlaybackEvent(event.payload));
        });

        return () => { unlisten.then((fn) => fn()); };
    }, []);
}

// Emit to backend
async function requestPause() {
    await emit('player-command', { action: 'pause' });
}
```

### Type Safety Across the Bridge

Types must match between Rust (`serde::Serialize`/`Deserialize`) and TypeScript. Use a shared type generation tool (`tauri-specta` or manual) to prevent drift.

```typescript
// Types mirror Rust structs exactly
// WHY: These must match the serde output from the Rust side.
// Update both sides together or use tauri-specta for generation.
interface Album {
    id: string;
    title: string;
    artist: string;
    track_count: number;       // snake_case matches Rust serde default
    duration_ms: number;
    cover_art_path: string | null;
}
```

Convention: keep Rust's `snake_case` field names in the TypeScript types that cross the IPC bridge. These are data transfer types, not UI types. Map to `camelCase` at the boundary if needed.

### Tauri API Over Web API

Use Tauri's APIs for OS-level operations, not browser equivalents.

```typescript
import { open } from '@tauri-apps/plugin-dialog';
import { readFile } from '@tauri-apps/plugin-fs';
import { appDataDir } from '@tauri-apps/api/path';

// Right: uses Tauri's native file picker
const selected = await open({ filters: [{ name: 'Audio', extensions: ['flac', 'mp3', 'ogg'] }] });

// Wrong: uses browser File API (no filesystem access in Tauri)
// document.querySelector('input[type=file]')
```

### Permissions

Tauri 2 uses capability-based permissions. Frontend code runs in a sandbox — backend commands must be explicitly allowed in `src-tauri/capabilities/`.

- Declare minimum required permissions per window
- Never grant blanket filesystem or shell access
- Scope file access to specific directories (e.g., music library path only)

---

## Testing

### Framework and Configuration

Vitest with happy-dom (faster than jsdom, sufficient for component tests). Fall back to jsdom per-file with `// @vitest-environment jsdom` when hitting API gaps.

```typescript
// vitest.config.ts
export default defineConfig({
    test: {
        environment: 'happy-dom',
        globals: true,
        setupFiles: ['./src/test/setup.ts'],
        coverage: { provider: 'v8' },
    },
});

// src/test/setup.ts
import '@testing-library/jest-dom/vitest';
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
});
```

### Test Organization

Colocated test files, same directory as source.

```
src/
  components/
    track-item.tsx
    track-item.test.tsx
  stores/
    player-store.ts
    player-store.test.ts
  lib/
    format-duration.ts
    format-duration.test.ts
```

### Naming

Test names describe behavior, not implementation.

```typescript
it('returns empty array when no tracks match filter')
it('pauses playback when track ends')
it('shows error message when import fails')
```

### Component Testing

React Testing Library — test behavior, not implementation details.

```typescript
import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';

it('calls onPlay when play button is clicked', async () => {
    const user = userEvent.setup();
    const onPlay = vi.fn();
    render(<TrackItem track={mockTrack} onPlay={onPlay} />);

    await user.click(screen.getByRole('button', { name: /play/i }));

    expect(onPlay).toHaveBeenCalledWith(mockTrack.id);
});

it('displays track duration in mm:ss format', () => {
    render(<TrackItem track={{ ...mockTrack, durationMs: 185000 }} />);
    expect(screen.getByText('3:05')).toBeInTheDocument();
});
```

### Mocking

Mock at module boundaries. Prefer `vi.spyOn` (type-safe, scoped) over `vi.mock` (hoisted, file-wide).

```typescript
// Preferred: vi.spyOn for targeted mocking
import * as api from '../api/tracks';

it('loads albums from backend', async () => {
    const spy = vi.spyOn(api, 'fetchAlbums').mockResolvedValue([mockAlbum]);
    render(<AlbumList />);
    await screen.findByText(mockAlbum.title);
    expect(spy).toHaveBeenCalledOnce();
});

// vi.mock for module replacement (when spyOn won't work)
// Use vi.hoisted to avoid hoisting pitfalls
const mockInvoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({
    invoke: mockInvoke,
}));

beforeEach(() => {
    mockInvoke.mockReset();
});

it('loads albums via Tauri IPC', async () => {
    mockInvoke.mockResolvedValue([mockAlbum]);
    const albums = await listAlbums();
    expect(mockInvoke).toHaveBeenCalledWith('list_albums');
});
```

### Store testing

Test Redux slices with a real store instance. No mocking the store.

```typescript
import { configureStore } from '@reduxjs/toolkit';
import { queueReducer, enqueue } from './queue-slice';

it('enqueues tracks in order', () => {
    const store = configureStore({ reducer: { queue: queueReducer } });
    store.dispatch(enqueue(trackA));
    store.dispatch(enqueue(trackB));
    expect(store.getState().queue.items).toEqual([trackA, trackB]);
});
```

### Resource Cleanup with `using`

`using` declarations (TS 5.2+) replace manual `afterEach` for test resources:

```typescript
it('queries the database', async () => {
    await using db = await TestDb.create();
    await db.seed(fixtures);
    const result = await db.query('SELECT count(*) FROM tracks');
    expect(result.count).toBe(5);
    // db automatically cleaned up — no afterEach needed
});
```

### No Snapshot Tests

Unless testing serialization formats. Snapshots are brittle, noisy in diffs, and tempt `--update` instead of investigation.

---

## Performance

### Transitions for Non-Urgent Updates

React 19: `startTransition` supports async callbacks. React tracks pending state automatically.

```typescript
import { useTransition } from 'react';

function SearchPage() {
    const [results, setResults] = useState<Track[]>([]);
    const [isPending, startTransition] = useTransition();

    function handleSearch(query: string) {
        startTransition(async () => {
            // Non-blocking — UI stays responsive during async work
            const data = await invoke<Track[]>('search_tracks', { query });
            setResults(data);
        });
    }

    return (
        <div>
            <SearchInput onChange={handleSearch} />
            {isPending && <Spinner />}
            <TrackList tracks={results} />
        </div>
    );
}
```

### Code Splitting

Route-level lazy loading with Suspense boundaries.

```typescript
const Settings = lazy(() => import('./pages/settings'));
const Library = lazy(() => import('./pages/library'));

function App() {
    return (
        <Suspense fallback={<PageSkeleton />}>
            <Routes>
                <Route path="/settings" element={<Settings />} />
                <Route path="/library" element={<Library />} />
            </Routes>
        </Suspense>
    );
}
```

### What Not to Optimize

- **Cheap computations:** Don't memoize string formatting, simple filters, or short array maps
- **Premature splitting:** Don't split every component into a lazy chunk — only route-level pages
- **Render count obsession:** React is fast. Profile before optimizing renders.
- **State normalization for small datasets:** Flat arrays with `.find()` are fine for <1000 items

### Tauri-Specific Performance

- Heavy computation belongs in Rust, not TypeScript — use commands for anything CPU-bound
- Large data transfers: prefer streaming events over single large `invoke` payloads
- Image handling: let Rust resize/thumbnail, send paths not blobs to the frontend

---

## Accessibility

### Semantic HTML First

Use the right element before reaching for ARIA.

```typescript
// Right: semantic elements with native behavior
<button onClick={handlePlay}>Play</button>
<input type="range" min={0} max={duration} value={progress} onChange={handleSeek} />
<time dateTime={`PT${seconds}S`}>{formatDuration(seconds)}</time>
<nav aria-label="Library navigation">...</nav>

// Wrong: div soup with ARIA bolted on
<div role="button" tabIndex={0} onClick={handlePlay}>Play</div>
```

### Custom Media Controls

Use `aria-valuetext` for human-readable slider values — screen readers announce "3:42 of 5:10" instead of raw numbers. Use `aria-pressed` for toggle buttons (shuffle, repeat, mute). Mark decorative icons `aria-hidden="true"`.

```typescript
// aria-valuetext: announce human-readable position
<input type="range" aria-label="Seek" aria-valuetext={`${formatTime(pos)} of ${formatTime(dur)}`} />

// aria-pressed: toggle state for buttons
<button aria-pressed={active} aria-label="Shuffle"><ShuffleIcon aria-hidden="true" /></button>

// aria-hidden: decorative icons inside labeled buttons
<button aria-label={isPlaying ? 'Pause' : 'Play'}><PauseIcon aria-hidden="true" /></button>
```

Full component examples: `reference/a11y-media-controls.tsx`

### Keyboard Navigation

- All interactive elements reachable via Tab
- Media controls respond to Space (toggle play), arrow keys (seek/volume)
- Escape closes modals/overlays and returns focus to trigger
- Focus trapping in modals and dialogs
- Visible focus indicators via `:focus-visible`

Use `:focus-visible` with `outline`, include `@media (forced-colors: active)` fallback. See `reference/a11y.css`.

### Color Contrast (WCAG AA)

| Element | Minimum Ratio |
|---------|---------------|
| Normal text (<18pt) | **4.5:1** |
| Large text (>=18pt / >=14pt bold) | **3:1** |
| UI components (borders, icons, focus rings) | **3:1** |
| Disabled controls | No requirement |

Verify both light and dark themes with browser DevTools accessibility panel.

### Live Regions for Dynamic Content

The live region element must exist in the DOM before content changes — only update its text content, never conditionally render the container.

```typescript
function TrackAnnouncer({ track }: { track: Track | null }) {
    return (
        <div role="status" aria-live="polite" aria-atomic="true" className="sr-only">
            {track ? `Now playing: ${track.title} by ${track.artist}` : ''}
        </div>
    );
}
```

Use `aria-live="polite"` (waits for screen reader to finish). Reserve `"assertive"` for time-critical alerts only (connection lost, errors).

Use the `.sr-only` pattern for screen-reader-only content. See `reference/a11y.css`.

### Motion and Preferences

Reduced motion does not mean zero animation. Remove *movement* (sliding, bouncing, pulsing), keep *fades* (opacity transitions). Use a `usePrefersReducedMotion` hook to react to the media query at runtime. See `reference/a11y-media-controls.tsx` and `reference/a11y.css`.

---

## Dependencies

**Preferred:**
- `@reduxjs/toolkit` + `react-redux` (state), `@tauri-apps/api` (IPC), `react-router` (routing)
- `@testing-library/react` + `@testing-library/user-event` (component tests)
- `date-fns` (date formatting — tree-shakeable)
- `valibot` or `zod` (schema validation at boundaries)

**Banned:**
- `moment` — dead project, massive bundle. Use `date-fns`.
- `lodash` (full) — use `lodash-es` individual imports or write the 3-line function
- `axios` — `fetch` is built in. For Tauri, use `invoke`.
- `enzyme` — dead, React Testing Library is the standard
- `styled-components` / `emotion` (runtime CSS-in-JS) — CSS modules or Tailwind instead
- `react-helmet` — React 19 hoists `<title>`, `<meta>`, `<link>` natively

**Policy:**
- Pin pre-1.0 packages to exact versions
- Wrap Tauri APIs in typed wrappers for testability
- `npm audit` in CI

---

## Anti-Patterns

1. **`any` type** — use `unknown` and narrow
2. **Class components** — function components only
3. **`React.FC`** — incorrect children inference, adds noise. Use plain function signature.
4. **`forwardRef`** — unnecessary in React 19. Pass `ref` as a prop.
5. **`<Context.Provider>`** — render `<Context value={...}>` directly in React 19
6. **Missing `key` prop in lists** — always provide stable keys, never array index
7. **`useEffect` for derived state** — compute during render instead
8. **`useEffect` for event handlers** — attach handlers directly, not via effect
9. **Floating promises** — await or void every async call
10. **Inline object/function creation in JSX** — causes unnecessary re-renders (when not using React Compiler)
11. **Prop drilling past 2 levels** — use Redux or context
12. **Selecting entire Redux state** — always use specific selectors
13. **Derived state in Redux** — use `createSelector` for memoized derivation
14. **`// @ts-ignore`** — fix the type error. Use `@ts-expect-error` with a reason if truly unavoidable.
15. **`as` type assertions** — use type guards and narrowing. `as` lies to the compiler.
16. **Barrel exports (`index.ts`)** — break tree-shaking and slow builds. Import directly from source files.
17. **Manual memoization with React Compiler** — remove `useMemo`/`useCallback`/`React.memo` when compiler is enabled. They're dead code.
18. **Browser APIs in Tauri** — use `@tauri-apps/api` for filesystem, dialogs, notifications. Browser APIs are sandboxed.
19. **Untyped IPC** — every `invoke` call must have explicit type parameters matching Rust types
20. **`enum`** — banned by `erasableSyntaxOnly`. Use `as const` objects or union types instead
21. **Manual deferred promise pattern** — use `Promise.withResolvers<T>()` (ES2024)
