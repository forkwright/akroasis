# TypeScript

> Additive to STANDARDS.md. Read that first. Everything here is TypeScript-specific.
>
> Target: TypeScript 5.x strict mode. React 19, Redux Toolkit, Vitest. Tauri 2 desktop app + web UI.
>
> **Key decisions:** TS 5.x strict, React 19, Redux Toolkit, Vitest + happy-dom, Vite, Biome, React Compiler, Tauri 2, branded types, RTK Query.

---

## Toolchain

- **Language:** TypeScript 5.x, `strict: true`. Zero `any` in new code
- **UI framework:** React 19 (function components only)
- **State:** Redux Toolkit (`@reduxjs/toolkit` + `react-redux`)
- **Testing:** Vitest + React Testing Library
- **Bundler:** Vite
- **Linter + Formatter:** Biome (see § Biome vs ESLint decision tree)
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
 `erasableSyntaxOnly` bans `enum`, `namespace`, and constructor parameter properties. This aligns with the type-stripping direction (Node `--experimental-strip-types`, Deno native TS). Use `as const` objects instead of enums.

### Biome vs ESLint

Default to Biome. Fall back to ESLint only when a required lint rule has no Biome equivalent.

| Criterion | Biome | ESLint + typescript-eslint + Prettier |
|-----------|-------|--------------------------------------|
| Speed | 10-100x faster. Written in Rust. | Slow on large monorepos. |
| Config surface | Single `biome.json`. Format + lint unified. | Three configs (`.eslintrc`, `.prettierrc`, tsconfig-eslint). |
| Plugin ecosystem | Limited. No custom rule API yet. | Extensive: `eslint-plugin-jsx-a11y`, `eslint-plugin-react-hooks`, `eslint-plugin-import-x`. |
| Type-aware rules | Partial. Growing. | Full via typescript-eslint with `parserOptions.project`. |

**Decision tree:**

1. Does the project need `eslint-plugin-jsx-a11y` or other plugin-only rules that Biome lacks? If yes, use ESLint.
2. Is the project a monorepo where lint speed blocks CI? Biome's speed advantage matters most here.
3. Otherwise, default to Biome. Fewer moving parts, faster feedback.

WHY: Biome eliminates the Prettier-ESLint conflict surface (formatting fights, overlapping rules) and runs in a fraction of the time. The tradeoff is ecosystem breadth. When Biome covers what you need, the simplicity wins.

When using ESLint, always pair with `typescript-eslint` flat config and `eslint-plugin-import-x` (maintained fork of `eslint-plugin-import`). Never use the deprecated `eslint-plugin-import`.

---

## Monorepo patterns

### Package manager: pnpm

pnpm workspaces for monorepos. WHY: strict dependency isolation (no phantom deps), disk-efficient via content-addressable store, workspace protocol for local packages.

```yaml
# pnpm-workspace.yaml
packages:
  - 'apps/*'
  - 'packages/*'
```

### Workspace protocol

Reference internal packages with `workspace:*`. pnpm resolves these to the local version at install time and replaces with the real version at publish time.

```jsonc
// apps/desktop/package.json
{
    "dependencies": {
        "@project/ui": "workspace:*",
        "@project/shared-types": "workspace:*"
    }
}
```

WHY: `workspace:*` makes internal dependencies explicit and prevents accidental resolution to a stale registry version. pnpm errors if the referenced package does not exist in the workspace.

### Package structure

```
monorepo/
├── pnpm-workspace.yaml
├── biome.json              # workspace-root config, shared by all packages
├── tsconfig.base.json      # shared compiler options, extended by each package
├── apps/
│   ├── desktop/            # Tauri app
│   │   ├── package.json
│   │   ├── tsconfig.json   # extends ../../tsconfig.base.json
│   │   └── src/
│   └── web/                # Web-only build
│       ├── package.json
│       ├── tsconfig.json
│       └── src/
└── packages/
    ├── ui/                 # Shared React components
    │   ├── package.json
    │   └── src/
    ├── shared-types/       # Types shared across apps (no runtime code)
    │   ├── package.json
    │   └── src/
    └── api-client/         # Typed API/IPC wrappers
        ├── package.json
        └── src/
```

Rules:

- **Shared config at root.** `tsconfig.base.json`, `biome.json`, and `.npmrc` live at the workspace root. Packages extend, never duplicate. WHY: config drift across packages causes inconsistent behavior that surfaces as CI-only failures.
- **`packages/` for libraries, `apps/` for deployables.** Libraries export typed APIs. Apps import them. Libraries never import from apps.
- **Types-only packages have zero runtime dependencies.** `shared-types` contains interfaces, branded types, and `as const` objects. No `lodash`, no `date-fns`, nothing that ships bytes.
- **Each package has its own `package.json`.** Even types-only packages. pnpm needs it for resolution.

### Shared tsconfig

```jsonc
// tsconfig.base.json (workspace root)
{
    "compilerOptions": {
        "target": "ES2024",
        "lib": ["ES2024", "DOM", "DOM.Iterable"],
        "module": "ESNext",
        "moduleResolution": "bundler",
        "strict": true,
        "noUncheckedIndexedAccess": true,
        "erasableSyntaxOnly": true,
        "skipLibCheck": true,
        "composite": true,
        "declaration": true,
        "declarationMap": true
    }
}

// apps/desktop/tsconfig.json
{
    "extends": "../../tsconfig.base.json",
    "compilerOptions": {
        "outDir": "dist",
        "rootDir": "src"
    },
    "include": ["src"],
    "references": [
        { "path": "../../packages/ui" },
        { "path": "../../packages/shared-types" }
    ]
}
```

`composite: true` and `references` enable incremental builds via `tsc --build`. WHY: without project references, `tsc` type-checks the entire workspace on every change. With references, only changed packages and their dependents are rechecked.

### Build ordering

pnpm respects dependency topology. `pnpm run --filter ...apps/desktop build` builds dependencies first.

```bash
# Build a specific app and everything it depends on
pnpm run --filter ...apps/desktop build

# Type-check entire workspace
pnpm run --recursive typecheck

# Run tests only for packages changed since main
pnpm run --filter "...[origin/main]" test
```

WHY: `--filter` with `...` prefix includes transitive dependencies. The `[origin/main]` syntax runs only changed packages, keeping CI fast as the monorepo grows.

### Anti-patterns

- **Hoisting everything to the root `node_modules`.** Use pnpm's strict mode (`.npmrc`: `shamefully-hoist=false`). Hoisting hides dependency errors that break in production.
- **Circular dependencies between packages.** If `@project/ui` imports from `@project/api-client` and vice versa, extract the shared type into `@project/shared-types`.
- **Different TypeScript versions across packages.** One `typescript` version in the root `package.json`. Packages inherit it.
- **Running `tsc` without `--build` in a monorepo.** `tsc --noEmit` does not respect project references. Use `tsc --build --noEmit` or per-package type-check scripts.

---

## Naming

See STANDARDS.md § Naming for universal conventions.

| Element | Convention | Example |
|---------|-----------|---------|
| Files | `kebab-case.ts` / `kebab-case.tsx` | `session-store.ts`, `track-item.tsx` |
| Functions / Variables | `camelCase` | `loadConfig`, `isPlaying` |
| Hooks | `use` prefix, `camelCase` | `usePlayer`, `useAlbum` |

- No `I` prefix on interfaces: `SessionConfig` not `ISessionConfig`
- Component files: one component per file, filename matches component name in kebab-case
- Event handlers: `onVerb` for props, `handleVerb` for implementations

---

## Type system

### Strict mode, zero `any`

`any` is banned in new code. Use `unknown` for truly unknown types and narrow with type guards.

```typescript
// Wrong
function process(data: any) { ... }

// Right
function process(data: unknown) {
    if (typeof data === 'string') { ... }
}
```

### Discriminated unions for state

```typescript
type PlayerState =
    | { status: 'loading' }
    | { status: 'playing'; track: Track; progress: number }
    | { status: 'error'; message: string };
```

### `as const` for literal types

```typescript
const ROUTES = {
    home: '/',
    player: '/player',
    settings: '/settings',
} as const;
```

### `satisfies` for type validation without widening

Use `satisfies` when you want type checking at definition without losing the narrower inferred type. Prefer over explicit annotation when the narrow type matters.

```typescript
// Type-checked AND preserves literal types
const ROUTES = {
    home: '/',
    player: '/player',
    settings: '/settings',
} as const satisfies Record<string, string>;
// typeof ROUTES.home is '/', not string

// Without satisfies: type widens to string
const ROUTES: Record<string, string> = { home: '/' };
// typeof ROUTES.home is string
```

Use `satisfies` when:
- Config objects where you want both validation and autocomplete on literal values
- Mapping objects that must conform to a shape but need preserved literals for type narrowing

### `const` type parameters

Use `const` type parameters when inference should preserve literal types from arguments.

```typescript
function createRoute<const T extends readonly string[]>(paths: T): T {
    return paths;
}
// Infers ['/', '/player'] not string[]
const routes = createRoute(['/', '/player']);
```

### `NoInfer<T>` for inference control

Prevents a type parameter position from influencing inference: forces inference from other call sites.

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

### `using` for resource management

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

### Inferred type predicates (TS 5.5+)

TypeScript now infers `x is T` return types automatically for narrowing functions. Delete explicit type predicates that only existed for `.filter()` workarounds.

```typescript
// Before 5.5: explicit predicate required
const valid = results.filter((r): r is Result => r !== undefined);

// After 5.5: inferred automatically
const valid = results.filter((r) => r !== undefined);
// Type: Result[]: narrowed correctly

// Works with discriminated unions
const clicks = actions.filter((a) => a.type === 'click');
// Inferred as { type: 'click'; x: number }[]
```

### Branded types for domain iDs

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

## Error handling

See STANDARDS.md § Error Handling for universal principles.

- `void` prefix for intentional fire-and-forget promises
- No floating promises: every `async` call must be awaited or explicitly voided
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

### Function components only

No class components. No `React.FC`: it has incorrect children inference and adds noise.

```typescript
// Standard component signature
function TrackItem({ track, onPlay }: TrackItemProps) {
    return <div>...</div>;
}
```

### Ref as prop (No `forwardRef`)

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
                // Cleanup: returned function runs on unmount or ref change
                return () => node?.pause();
            }}
            src={src}
        />
    );
}
```

### Context as provider

React 19 renders `<Context>` directly: no more `.Provider`.

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

### `use()` hook

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

### Actions and form handling

React 19 Actions integrate with transitions for async form operations.

```typescript
import { useActionState, useOptimistic, useTransition } from 'react';

// useActionState: form action with pending state
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

// useOptimistic: instant UI feedback before server confirmation
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

### Document metadata

React 19 hoists `<title>`, `<meta>`, and `<link>` to `<head>` automatically.

```typescript
function AlbumPage({ album }: { album: Album }) {
    return (
        <>
            <title>{album.name} | Harmonia</title>
            <meta name="description" content={album.description} />
            <AlbumView album={album} />
        </>
    );
}
```

### Hooks rules

- `useEffect`: cleanup function required for subscriptions, timers, listeners
- Custom hooks extract reusable logic: `usePlayer()`, `useAlbum(id)`
- Dependency arrays must be exhaustive: lint enforces this
- Prefer `useTransition` for non-urgent state updates over raw `setState`

### `useFormStatus` for child components

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

### React compiler

Stable since October 2025. Ships as a Babel plugin. Auto-memoizes components, values, and callbacks at build time: including conditional paths that manual memoization cannot cover.

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

// Opt out (any mode: escape hatch)
function LegacyWidget() {
    "use no memo";
    // compiler leaves this alone
}
```

**When to use `"use no memo"`:**

The compiler assumes React's Rules of Hooks and pure render functions. Opt out when the component intentionally violates these assumptions:

| Scenario | Why opt out |
|----------|-------------|
| Component reads from a mutable external store without `useSyncExternalStore` | Compiler may cache a stale read. The component must re-render on every call to observe the mutation. |
| Component relies on render-time side effects for measurement (e.g., `getBoundingClientRect` in render) | Compiler may skip the re-execution if inputs appear unchanged. |
| Third-party library passes mutable refs as props and expects them to be read fresh each render | Compiler cannot track mutations to ref objects passed from outside. |
| Gradual migration: component has not yet been audited for Rules of React compliance | Safer to exclude than to ship memoization bugs. Audit and remove `"use no memo"` as part of migration. |

Do not use `"use no memo"` for:
- Performance tuning (the compiler memoizes more granularly than you can manually)
- Components that "seem to break" without investigation (diagnose the actual rule violation first)

**What the compiler can and cannot infer:**

| Can infer | Cannot infer |
|-----------|-------------|
| Memoization of JSX elements, hook return values, and intermediate variables | Side effects hidden behind opaque function calls (e.g., a function that mutates a closed-over variable) |
| Dependency tracking for conditional branches (better than manual `useMemo` deps) | Mutations to objects shared across components via refs or module-level variables |
| Callback stability for event handlers defined inline | Whether a third-party hook follows the Rules of Hooks internally |
| Granular re-render skipping at the expression level, not just the component level | Effects of `eval()`, dynamic `import()`, or Proxy-based reactivity systems |

WHY: the compiler operates on static analysis of the component's source code. It trusts that functions are pure and hooks follow the rules. When those assumptions hold, the compiler produces strictly better memoization than any manual approach. When they do not hold, `"use no memo"` is the correct escape hatch, not a performance knob.

**Manual memoization rules:**

- **With compiler:** Do not write `useMemo`, `useCallback`, `React.memo`. The compiler handles it. Manual memoization is dead code.
- **Without compiler:** Use `useMemo` for expensive computations only. Use `useCallback` only when passing to memoized children. Use `React.memo` for components that re-render frequently with same props.
- **Never:** Memoize cheap operations. The memoization overhead exceeds the savings.

---

## State management (Redux toolkit)

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

Immer runs inside `createSlice` reducers: mutative syntax (`state.track = action.payload`) produces immutable updates. Don't use Immer outside slices.

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

### RTK query for server state

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

- **Direct state mutation outside reducers**: Immer only runs inside `createSlice` reducers; external mutation creates bugs
- **Business logic in components**: thunks and slices handle logic; components dispatch and select
- **Overusing Redux**: modal open/closed, form field values, and per-component state belong in `useState`
- **Missing `createSelector` for derived state**: recomputes on every render without memoization
- **Non-serializable values in state**: no functions, class instances, or `Date` objects. Use ISO strings for timestamps.
- **Server state in Redux slices**: use RTK Query for backend data (caching, refetch, invalidation)
- **Untyped hooks**: always use the typed `useAppDispatch`/`useAppSelector`, never raw `useDispatch`/`useSelector`

---

## Tauri 2 IPC

### Commands (Frontend → backend)

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

### Events (Backend → frontend)

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

### Type safety across the bridge

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

### Tauri API over web API

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

Tauri 2 uses capability-based permissions. Frontend code runs in a sandbox: backend commands must be explicitly allowed in `src-tauri/capabilities/`.

- Declare minimum required permissions per window
- Never grant blanket filesystem or shell access
- Scope file access to specific directories (e.g., music library path only)

---

## Layered architecture

### Layer model

```
Request → Handler → Service → Repository → Data Source
                ↑           ↑            ↑
             Validation   Business     Persistence
             + DTO map    logic        abstraction
```

Each layer depends only on the layer below it. No skipping layers. No upward imports.

WHY: layering isolates change. A database migration touches the repository layer. A new business rule touches the service layer. A UI redesign touches the handler layer. Without layering, a database change propagates through the entire codebase.

### Layer responsibilities

| Layer | Owns | Does NOT own |
|-------|------|-------------|
| **Handler** (component/route) | Input validation, DTO→domain mapping, error→UI mapping, loading/error states | Business rules, data fetching logic |
| **Service** (hook/thunk) | Business rules, orchestration, domain validation | UI concerns, persistence details |
| **Repository** (API client/IPC wrapper) | Data access, serialization, caching strategy | Business rules, UI concerns |

### Validation placement

Validate at each boundary for what that boundary owns:

```typescript
// Handler layer: validate shape and format
function ImportPage() {
    function handleSubmit(formData: FormData) {
        const path = formData.get('path');
        if (typeof path !== 'string' || path.trim() === '') {
            return; // UI validation: shape
        }
        dispatch(importLibrary(path));
    }
    // ...
}

// Service layer: validate business rules
export const importLibrary = createAsyncThunk(
    'library/import',
    async (path: string, { rejectWithValue }) => {
        if (!path.endsWith('/') && !path.includes('.')) {
            return rejectWithValue({ kind: 'invalid_path', path });
        }
        return invoke<ImportReport>('import_library', { path });
    },
);

// Repository layer: validate wire format
function parseAlbumResponse(raw: unknown): Album {
    return AlbumSchema.parse(raw); // valibot/zod at the boundary
}
```

WHY: handler validation prevents invalid data from reaching the service. Service validation enforces business invariants. Repository validation ensures wire data matches expected shapes. Each layer trusts its inputs only because the layer above validated them.

### DTO flow

Domain types live in the service/domain layer. Wire types (DTOs) live at boundaries. Map between them explicitly.

```typescript
// Wire type: matches backend serde output (snake_case)
interface AlbumDto {
    id: string;
    title: string;
    artist_name: string;
    track_count: number;
    duration_ms: number;
}

// Domain type: used throughout the frontend (camelCase)
interface Album {
    id: AlbumId;
    title: string;
    artistName: string;
    trackCount: number;
    durationMs: number;
}

// Map at the repository boundary
function toAlbum(dto: AlbumDto): Album {
    return {
        id: createAlbumId(dto.id),
        title: dto.title,
        artistName: dto.artist_name,
        trackCount: dto.track_count,
        durationMs: dto.duration_ms,
    };
}
```

WHY: if the backend renames a field, exactly one mapping function changes. Without the boundary map, snake_case field names leak through every component and hook. Branded types (`AlbumId`) are constructed at this boundary, enforcing type safety from the earliest possible point.

### File organization by layer

```
src/
├── domain/             # Types, branded IDs, business constants
│   ├── album.ts
│   └── track.ts
├── services/           # Thunks, business logic, orchestration
│   ├── library-service.ts
│   └── playback-service.ts
├── repositories/       # IPC wrappers, API clients, schema validation
│   ├── album-repository.ts
│   └── track-repository.ts
├── store/              # Redux slices, selectors, store config
│   ├── store.ts
│   ├── slices/
│   └── hooks.ts
├── components/         # React components (handler layer)
│   ├── album-list.tsx
│   └── track-item.tsx
└── pages/              # Route-level components with Suspense boundaries
    ├── library-page.tsx
    └── settings-page.tsx
```

Components import from `services/` and `store/`. Services import from `repositories/` and `domain/`. Repositories import from `domain/` only. No reverse dependencies.

---

## Testing

See TESTING.md for all testing principles (naming, isolation, coverage, test data, property testing).

TypeScript-specific framework choices and patterns:

### Framework and configuration

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

### Component testing

React Testing Library for component tests.

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
```

### Mocking

Prefer `vi.spyOn` (type-safe, scoped) over `vi.mock` (hoisted, file-wide). Use `vi.hoisted` when `vi.mock` is needed.

### Store testing

Test Redux slices with a real store instance. No mocking the store.

### Resource cleanup with `using`

`using` declarations (TS 5.2+) replace manual `afterEach` for test resources.

### No snapshot tests

Unless testing serialization formats. Snapshots are brittle, noisy in diffs, and tempt `--update` instead of investigation.

---

## Performance

### Transitions for non-Urgent updates

React 19: `startTransition` supports async callbacks. React tracks pending state automatically.

```typescript
import { useTransition } from 'react';

function SearchPage() {
    const [results, setResults] = useState<Track[]>([]);
    const [isPending, startTransition] = useTransition();

    function handleSearch(query: string) {
        startTransition(async () => {
            // Non-blocking: UI stays responsive during async work
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

### Code splitting

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

### What not to optimize

- **Cheap computations:** Don't memoize string formatting, filters, or short array maps
- **Premature splitting:** Don't split every component into a lazy chunk: only route-level pages
- **Render count obsession:** React is fast. Profile before optimizing renders.
- **State normalization for small datasets:** Flat arrays with `.find()` are fine for <1000 items

### Tauri-Specific performance

- Heavy computation belongs in Rust, not TypeScript: use commands for anything CPU-bound
- Large data transfers: prefer streaming events over single large `invoke` payloads
- Image handling: let Rust resize/thumbnail, send paths not blobs to the frontend

### Core Web Vitals

WHY: Core Web Vitals are Google's user-experience metrics. They measure what users actually feel: loading speed, interactivity, and visual stability. Poor scores degrade user experience and search ranking for web builds. Tauri desktop builds benefit from the same discipline since the rendering engine is a webview.

#### Metrics and budgets

| Metric | What it measures | Good | Poor | Budget |
|--------|-----------------|------|------|--------|
| **LCP** (Largest Contentful Paint) | When the main content finishes rendering | ≤ 2.5s | > 4.0s | ≤ 1.5s for app shells |
| **INP** (Interaction to Next Paint) | Responsiveness to user input (replaces FID) | ≤ 200ms | > 500ms | ≤ 100ms for media controls |
| **CLS** (Cumulative Layout Shift) | Visual stability during load and interaction | ≤ 0.1 | > 0.25 | ≤ 0.05 |
| **FCP** (First Contentful Paint) | When the first content pixel renders | ≤ 1.8s | > 3.0s | ≤ 1.0s |
| **TTFB** (Time to First Byte) | Server/backend response time | ≤ 800ms | > 1800ms | ≤ 200ms for local IPC |

INP replaced First Input Delay (FID) in March 2024. INP measures all interactions across the page lifecycle, not just the first one. Optimize for INP.

#### Measurement

Use the `web-vitals` library in production to capture real user metrics. Do not rely solely on Lighthouse: lab scores miss real-world variance.

```typescript
import { onLCP, onINP, onCLS } from 'web-vitals';

function reportMetric(metric: { name: string; value: number; id: string }) {
    // Send to your analytics/observability pipeline
    navigator.sendBeacon('/api/vitals', JSON.stringify(metric));
}

onLCP(reportMetric);
onINP(reportMetric);
onCLS(reportMetric);
```

For Tauri desktop builds, log vitals to the Rust backend via `invoke` instead of `sendBeacon`.

#### Common LCP violations and fixes

| Violation | Fix |
|-----------|-----|
| Render-blocking CSS/JS | Inline critical CSS. Defer non-critical scripts. |
| Unoptimized hero images | Use `width`/`height` attributes (prevents layout shift). Serve modern formats (WebP/AVIF). Lazy-load below-the-fold images only. |
| Client-side data fetch before render | Prefetch data in the route loader or start the fetch before rendering the component (see `use()` hook pattern). |
| Large JavaScript bundles | Code-split at route boundaries. Audit with `npx vite-bundle-visualizer`. |

#### Common CLS violations and fixes

| Violation | Fix |
|-----------|-----|
| Images without dimensions | Always set `width` and `height` on `<img>` elements. Use CSS `aspect-ratio` for responsive images. |
| Dynamically injected content | Reserve space with skeleton placeholders. Never insert content above the user's viewport. |
| Web fonts causing FOUT/FOIT | Use `font-display: swap` with a matching fallback font. Use `size-adjust` on the fallback to minimize shift. |
| Animations that trigger layout | Animate only `transform` and `opacity`. Never animate `width`, `height`, `top`, `left`, or `margin`. |

#### Common INP violations and fixes

| Violation | Fix |
|-----------|-----|
| Long-running event handlers | Move heavy work into `startTransition()`. Yield to the main thread with `scheduler.yield()` (or `setTimeout(0)` as fallback). |
| Synchronous Redux dispatches blocking paint | Wrap non-urgent dispatches in `startTransition`. Keep urgent interactions (play/pause, seek) synchronous. |
| Large component trees re-rendering on interaction | React Compiler handles this when enabled. Without compiler, profile with React DevTools and memoize the hot path. |

#### Performance budgets in CI

Track bundle size and vitals in CI. Fail the build if budgets are exceeded.

```typescript
// vite.config.ts — build size warning
export default defineConfig({
    build: {
        rollupOptions: {
            output: {
                // Warn if any chunk exceeds 250KB gzipped
                experimentalMinChunkSize: 1000,
            },
        },
    },
});
```

Use `bundlesize` or Vite's built-in `build.chunkSizeWarningLimit` as a gate. Track the main chunk, vendor chunk, and total JS payload independently.

---

## Accessibility

### Semantic HTML first

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

### Custom media controls

Use `aria-valuetext` for human-readable slider values: screen readers announce "3:42 of 5:10" instead of raw numbers. Use `aria-pressed` for toggle buttons (shuffle, repeat, mute). Mark decorative icons `aria-hidden="true"`.

```typescript
// aria-valuetext: announce human-readable position
<input type="range" aria-label="Seek" aria-valuetext={`${formatTime(pos)} of ${formatTime(dur)}`} />

// aria-pressed: toggle state for buttons
<button aria-pressed={active} aria-label="Shuffle"><ShuffleIcon aria-hidden="true" /></button>

// aria-hidden: decorative icons inside labeled buttons
<button aria-label={isPlaying ? 'Pause' : 'Play'}><PauseIcon aria-hidden="true" /></button>
```

Full component examples: `reference/a11y-media-controls.tsx`

### Keyboard navigation

- All interactive elements reachable via Tab
- Media controls respond to Space (toggle play), arrow keys (seek/volume)
- Escape closes modals/overlays and returns focus to trigger
- Focus trapping in modals and dialogs
- Visible focus indicators via `:focus-visible`

Use `:focus-visible` with `outline`, include `@media (forced-colors: active)` fallback. See `reference/a11y.css`.

### Color contrast (WCAG aA)

| Element | Minimum Ratio |
|---------|---------------|
| Normal text (<18pt) | **4.5:1** |
| Large text (>=18pt / >=14pt bold) | **3:1** |
| UI components (borders, icons, focus rings) | **3:1** |
| Disabled controls | No requirement |

Verify both light and dark themes with browser DevTools accessibility panel.

### Live regions for dynamic content

The live region element must exist in the DOM before content changes: only update its text content, never conditionally render the container.

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

### Motion and preferences

Reduced motion does not mean zero animation. Remove *movement* (sliding, bouncing, pulsing), keep *fades* (opacity transitions). Use a `usePrefersReducedMotion` hook to react to the media query at runtime. See `reference/a11y-media-controls.tsx` and `reference/a11y.css`.

---

## Dependencies

**Preferred:**
- `@reduxjs/toolkit` + `react-redux` (state), `@tauri-apps/api` (IPC), `react-router` (routing)
- `@testing-library/react` + `@testing-library/user-event` (component tests)
- `date-fns` (date formatting: tree-shakeable)
- `valibot` or `zod` (schema validation at boundaries)

**Banned:**
- `moment`: dead project, massive bundle. Use `date-fns`.
- `lodash` (full): use `lodash-es` individual imports or write the 3-line function
- `axios`: `fetch` is built in. For Tauri, use `invoke`.
- `enzyme`: dead, React Testing Library is the standard
- `styled-components` / `emotion` (runtime CSS-in-JS): CSS modules or Tailwind instead
- `react-helmet`: React 19 hoists `<title>`, `<meta>`, `<link>` natively

**Policy:**
- Pin pre-1.0 packages to exact versions
- Wrap Tauri APIs in typed wrappers for testability
- `npm audit` in CI

---

## Anti-Patterns

1. **`any` type**: use `unknown` and narrow
2. **Class components**: function components only
3. **`React.FC`**: incorrect children inference, adds noise. Use plain function signature.
4. **`forwardRef`**: unnecessary in React 19. Pass `ref` as a prop.
5. **`<Context.Provider>`**: render `<Context value={...}>` directly in React 19
6. **Missing `key` prop in lists**: always provide stable keys, never array index
7. **`useEffect` for derived state**: compute during render instead
8. **`useEffect` for event handlers**: attach handlers directly, not via effect
9. **Floating promises**: await or void every async call
10. **Inline object/function creation in JSX**: causes unnecessary re-renders (when not using React Compiler)
11. **Prop drilling past 2 levels**: use Redux or context
12. **Selecting entire Redux state**: always use specific selectors
13. **Derived state in Redux**: use `createSelector` for memoized derivation
14. **`// @ts-ignore`**: fix the type error. Use `@ts-expect-error` with a reason if truly unavoidable.
15. **`as` type assertions**: use type guards and narrowing. `as` lies to the compiler.
16. **Barrel exports (`index.ts`)**: break tree-shaking and slow builds. Import directly from source files.
17. **Manual memoization with React Compiler**: remove `useMemo`/`useCallback`/`React.memo` when compiler is enabled. They're dead code.
18. **Browser APIs in Tauri**: use `@tauri-apps/api` for filesystem, dialogs, notifications. Browser APIs are sandboxed.
19. **Untyped IPC**: every `invoke` call must have explicit type parameters matching Rust types
20. **`enum`**: banned by `erasableSyntaxOnly`. Use `as const` objects or union types instead
21. **Manual deferred promise pattern**: use `Promise.withResolvers<T>()` (ES2024)
