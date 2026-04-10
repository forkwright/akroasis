# Kotlin

> Additive to STANDARDS.md. Read that first. Everything here is Kotlin-specific.
>
> Target: Kotlin 2.x (K2 compiler), Android (Jetpack Compose, Hilt, Room), Kotlin Multiplatform (KMP), Compose Multiplatform. Akroasis media player.
>
> **Key decisions:** K2 compiler, Compose Multiplatform + Material 3, Hilt DI (Android), Koin (KMP), Room DB, KSP (not kapt), kotlinx.serialization, StateFlow (not LiveData), coroutines, Navigation (type-safe), expect/actual for platform abstraction.

---

## Toolchain

- **Language:** Kotlin 2.x (K2 compiler, default since 2.0)
- **UI:** Jetpack Compose with Material 3
- **DI:** Hilt
- **Database:** Room
- **Annotation processing:** KSP (not kapt: kapt is maintenance-only)
- **Serialization:** kotlinx.serialization
- **Build/validate:**
  ```bash
  cd android && ./gradlew build && ./gradlew test
 ```

---

## Naming

See STANDARDS.md § Naming for universal conventions.

| Element | Convention | Example |
|---------|-----------|---------|
| Files | `PascalCase.kt` | `PlayerViewModel.kt` |
| Functions / Properties | `camelCase` | `loadAlbum`, `isPlaying` |
| Packages | `lowercase` | `app.akroasis.ui.player` |
| Composables | `PascalCase` (Compose convention) | `TrackItem`, `PlayerScreen` |
| StateFlows | `_camelCase` (private) / `camelCase` (public) | `_uiState` / `uiState` |

---

## Type system

### Sealed hierarchies for state

```kotlin
sealed interface PlayerUiState {
    data object Loading : PlayerUiState
    data class Playing(val track: Track, val progress: Float) : PlayerUiState
    data class Error(val message: String) : PlayerUiState
}
```

Exhaustive `when` expressions over sealed types. The compiler enforces all branches.

`data object` for sealed hierarchy leaves that carry no data: provides proper `toString()`, `equals()`, `hashCode()`.

### Data classes for value types

```kotlin
data class Track(
    val id: Long,
    val title: String,
    val duration: Duration,
    val albumId: Long,
)
```

### Value classes

Use `value class` for type-safe wrappers with zero allocation overhead:

```kotlin
@JvmInline
value class TrackId(val value: Long)

@JvmInline
value class AlbumId(val value: Long)
```

Single-property restriction. Boxing occurs when used as a generic type parameter or through an interface. `@JvmInline` required for JVM targets.

### Null safety

Kotlin's type system distinguishes nullable (`String?`) from non-null (`String`). Use it. Never use `!!` without a clear invariant justification. Prefer `?.let`, `?:`, `requireNotNull()`.

---

## Error handling

See STANDARDS.md § Error Handling for universal principles.

### `Result<T>` for operations that can fail

```kotlin
fun loadAlbum(id: Int): Result<Album> {
    return runCatching {
        repository.getAlbum(id)
    }
}
```

### Sealed error hierarchies

```kotlin
sealed class AppError {
    data class NotFound(val id: String) : AppError()
    data class Network(val cause: Throwable) : AppError()
    data class Unauthorized(val reason: String) : AppError()
}
```

### Rules

- `runCatching { }` for concise error handling where appropriate
- Propagate errors to the ViewModel: let the UI layer decide presentation

---

## Async & concurrency

### Coroutines with structured concurrency

- `viewModelScope` for ViewModel-scoped coroutines
- `Dispatchers.IO` for I/O operations, never `Dispatchers.Main` for blocking work
- `withContext()` for dispatcher switching
- **Never `GlobalScope`**: use structured concurrency always

```kotlin
fun loadAlbum(id: Int) {
    viewModelScope.launch {
        _uiState.value = AlbumUiState.Loading
        val result = withContext(Dispatchers.IO) {
            albumRepository.getAlbum(id)
        }
        _uiState.value = result.fold(
            onSuccess = { AlbumUiState.Loaded(it) },
            onFailure = { AlbumUiState.Error(it.message ?: "Unknown error") },
        )
    }
}
```

### StateFlow for reactive state

- `MutableStateFlow` private, expose as `StateFlow`
- `stateIn` with `SharingStarted.WhileSubscribed(5000)` for flow collection
- No `LiveData` in new code: `StateFlow` everywhere

```kotlin
private val _uiState = MutableStateFlow<PlayerUiState>(PlayerUiState.Loading)
val uiState: StateFlow<PlayerUiState> = _uiState.asStateFlow()
```

---

## Compose

### State hoisting

Composables are stateless by default. State lives in the ViewModel. Pass state down, emit events up.

```kotlin
@Composable
fun TrackItem(
    track: Track,
    isPlaying: Boolean,
    onTrackClick: (Track) -> Unit,
    modifier: Modifier = Modifier,
) {
    // stateless: all state passed in
}
```

### Rules

- `remember` / `rememberSaveable` for local ephemeral state only
- `@Preview` on all reusable composables
- Material 3 design system
- Extract reusable composables into separate files

See § Compose Multiplatform for cross-platform UI patterns.

---

## Dependency injection

**Hilt** for all DI.

- `@HiltViewModel` on all ViewModels
- `@Inject constructor`: no manual instantiation
- Module bindings in `di/` package
- `@Singleton` for app-scoped, `@ViewModelScoped` for ViewModel-scoped

```kotlin
@HiltViewModel
class PlayerViewModel @Inject constructor(
    private val playbackRepository: PlaybackRepository,
    private val savedStateHandle: SavedStateHandle,
) : ViewModel()
```

---

## Serialization

**kotlinx.serialization** for all new code. Compiler plugin: no reflection.

```kotlin
@Serializable
data class TrackResponse(
    val id: Long,
    val title: String,
    val duration: Long,
    @SerialName("album_id") val albumId: Long,
)
```

- Required for Kotlin Multiplatform
- Handles `data object`, sealed classes, contextual serialization
- **Gson is legacy**: does not understand Kotlin nullability, default parameters, or value classes. Migrate away.
- Moshi acceptable for existing Android/JVM-only projects

---

## Testing

See TESTING.md for all testing principles (naming, isolation, coverage, test data, property testing).

Kotlin-specific framework choices:

- **Framework:** JUnit 5 + Turbine (for Flow testing)
- **Property testing:** Kotest property testing
- **Coroutine testing:** `runTest { }` with `TestDispatcher`
- **Compose testing:** `createComposeRule()` for UI tests
- **Mocking:** MockK at interface boundaries

---

## Architecture

MVVM with unidirectional data flow:

```
app/src/main/java/app/akroasis/
├── data/          : repositories, data sources, Room DAOs
├── domain/        : use cases, domain models
├── ui/            : screens, composables, ViewModels
├── di/            : Hilt modules
└── util/          : shared utilities
```

- ViewModel exposes `StateFlow<UiState>`, never `LiveData`
- UI observes state, emits events
- Repository is the single source of truth for data
- Use cases encapsulate business logic between ViewModel and Repository

---

## Kotlin Flow

StateFlow basics are in § Async & concurrency. This section covers advanced Flow patterns.

### Operators

Use `map`, `filter`, `combine`, `flatMapLatest` for declarative stream transformation. Avoid `collect` inside `collect` — compose flows with operators instead.

WHY: Nested collection creates nested coroutines that are hard to cancel correctly and easy to leak. Operators keep the flow pipeline flat and cancellation-safe.

```kotlin
// Wrong: nested collection
viewModelScope.launch {
    searchQuery.collect { query ->
        repository.search(query).collect { results ->
            _uiState.value = results
        }
    }
}

// Right: flatMapLatest cancels the previous inner flow on new emission
searchQuery
    .debounce(300)
    .flatMapLatest { query -> repository.search(query) }
    .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), emptyList())
```

### `stateIn` and `shareIn`

Convert cold flows to hot shared flows at the ViewModel layer. Never expose cold flows from a ViewModel — every collector would restart the upstream.

WHY: Cold flows re-execute their producer on each collection. A Room query flow collected by two composables would run two separate database queries. `stateIn` shares one upstream and replays the latest value.

```kotlin
val albums: StateFlow<List<Album>> = albumRepository.observeAll()
    .stateIn(
        scope = viewModelScope,
        started = SharingStarted.WhileSubscribed(5000),
        initialValue = emptyList(),
    )
```

`WhileSubscribed(5000)`: keeps the upstream alive for 5 seconds after the last subscriber disappears. Survives configuration changes without restarting the query. `Eagerly` and `Lazily` exist but rarely fit UI use cases.

### `callbackFlow` for bridging imperative APIs

Wrap callback-based APIs (media players, sensors, platform listeners) in `callbackFlow`:

```kotlin
fun observePlaybackState(player: MediaPlayer): Flow<PlaybackState> = callbackFlow {
    val listener = object : Player.Listener {
        override fun onPlaybackStateChanged(state: Int) {
            trySend(state.toPlaybackState())
        }
    }
    player.addListener(listener)
    awaitClose { player.removeListener(listener) }
}
```

WHY: `callbackFlow` provides a `SendChannel` with proper lifecycle — `awaitClose` guarantees cleanup runs when the flow collector cancels. Raw channels miss this guarantee.

### Flow testing

Use Turbine for deterministic flow assertions:

```kotlin
@Test
fun `search emits results after debounce`() = runTest {
    val viewModel = SearchViewModel(FakeRepository())
    viewModel.results.test {
        viewModel.onQueryChanged("beethoven")
        awaitItem() // initial empty
        awaitItem() // search results
        cancelAndIgnoreRemainingEvents()
    }
}
```

Never use `first()` or `take(1)` in tests — they silently swallow timing issues. Turbine makes assertion order explicit and fails on unexpected emissions.

---

## Navigation

### Type-safe navigation (Navigation 2.8+)

Use Kotlin serializable classes as route definitions. String-based routes are banned in new code.

WHY: String routes are stringly-typed — typos compile, missing arguments are runtime crashes, and refactoring is grep-and-pray. Serializable route objects give compile-time safety for arguments and destinations.

```kotlin
@Serializable
data class AlbumRoute(val albumId: Long)

@Serializable
data object PlayerRoute

@Serializable
data object LibraryRoute
```

### NavHost setup

```kotlin
@Composable
fun AppNavHost(
    navController: NavHostController,
    modifier: Modifier = Modifier,
) {
    NavHost(
        navController = navController,
        startDestination = LibraryRoute,
        modifier = modifier,
    ) {
        composable<LibraryRoute> {
            LibraryScreen(onAlbumClick = { navController.navigate(AlbumRoute(it.id)) })
        }
        composable<AlbumRoute> { backStackEntry ->
            val route = backStackEntry.toRoute<AlbumRoute>()
            AlbumScreen(albumId = route.albumId)
        }
        composable<PlayerRoute> {
            PlayerScreen()
        }
    }
}
```

### Rules

- One `NavHost` per app. Nested navigation uses `navigation()` builder for sub-graphs, not nested `NavHost`.
- `NavController` lives in the top-level composable or Activity. Never pass it into ViewModels. ViewModels emit navigation events; the UI layer acts on them.
- Deep links: define on the route class with `@Serializable` + `deepLinks` parameter in `composable()`.
- Back stack: use `popUpTo` with `inclusive` to prevent stack accumulation on bottom navigation tabs.

WHY (NavController out of ViewModel): Navigation is a UI concern. ViewModels survive configuration changes — if they hold a NavController reference, it points to a destroyed Activity's controller after rotation. Emit a one-shot event (`Channel<NavigationEvent>`) instead.

```kotlin
// In ViewModel
private val _navigation = Channel<NavigationEvent>(Channel.BUFFERED)
val navigation = _navigation.receiveAsFlow()

fun onAlbumSelected(albumId: Long) {
    viewModelScope.launch {
        _navigation.send(NavigationEvent.GoToAlbum(albumId))
    }
}

// In composable
LaunchedEffect(Unit) {
    viewModel.navigation.collect { event ->
        when (event) {
            is NavigationEvent.GoToAlbum -> navController.navigate(AlbumRoute(event.albumId))
        }
    }
}
```

---

## Compose Multiplatform

### Platform targets

| Target | Status | Notes |
|--------|--------|-------|
| Android | Stable | Primary target, full Jetpack integration |
| iOS | Stable (CMP 1.8+) | UIKit interop via `UIKitView` |
| Desktop (JVM) | Stable | Swing/AWT interop via `SwingPanel` |
| Web (Wasm) | Beta | `@ExperimentalWasmDsl`, limited interop |

### Project structure

```
shared/
├── commonMain/       : shared UI composables, ViewModels, expect declarations
├── androidMain/      : Android-specific actual implementations
├── iosMain/          : iOS-specific actual implementations
├── desktopMain/      : Desktop-specific actual implementations
└── commonTest/       : shared tests
```

All UI composables go in `commonMain` unless they depend on platform APIs. Platform-specific composables use `expect`/`actual` (see § Kotlin Multiplatform).

### Shared composables

Write composables against the Compose Multiplatform API, not Jetpack Compose directly. The API surface is nearly identical but the import paths differ.

WHY: `androidx.compose.*` imports are Android-only. `org.jetbrains.compose.*` works across all targets. Mixing them causes compilation failures on non-Android targets.

```kotlin
// commonMain — works everywhere
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable

@Composable
fun Greeting(name: String, modifier: Modifier = Modifier) {
    Text(text = "Hello, $name", modifier = modifier)
}
```

Material 3 is available cross-platform via the `compose.material3` dependency in Compose Multiplatform.

### Platform-specific UI

Use `expect`/`actual` for composables that wrap platform-native views:

```kotlin
// commonMain
@Composable
expect fun VideoPlayer(uri: String, modifier: Modifier = Modifier)

// androidMain
@Composable
actual fun VideoPlayer(uri: String, modifier: Modifier) {
    AndroidView(factory = { context -> ExoPlayerView(context).apply { load(uri) } })
}

// iosMain
@Composable
actual fun VideoPlayer(uri: String, modifier: Modifier) {
    UIKitView(factory = { AVPlayerView().apply { load(uri) } })
}
```

### Resources

Use `Res` from Compose Multiplatform resources API (`org.jetbrains.compose.resources`). Place resources in `commonMain/composeResources/`.

WHY: Android's `R` class does not exist on other platforms. The Compose Multiplatform resources API generates type-safe accessors that work everywhere.

```kotlin
// commonMain
Image(
    painter = painterResource(Res.drawable.album_art),
    contentDescription = "Album art",
)
Text(text = stringResource(Res.string.app_name))
```

### Rules

- Test shared composables in `commonTest` with `@OptIn(ExperimentalTestApi::class) runComposeUiTest { }`.
- Image loading: use Coil 3 (multiplatform) or Kamel. Glide and Picasso are Android-only.
- Lifecycle: use `androidx.lifecycle` ViewModel and `Lifecycle` from the multiplatform artifact (`lifecycle-viewmodel-compose`, `lifecycle-runtime-compose`).
- Window management: `application { Window() }` on desktop, standard Activity on Android, no window concept on iOS (UIKit manages it).

---

## Kotlin Multiplatform (KMP)

### Architecture

Shared business logic in `commonMain`. Platform code only for platform APIs. The boundary is `expect`/`actual`.

```
:shared              (KMP library)
├── commonMain/      : domain models, use cases, repositories (interfaces), ViewModels
├── androidMain/     : Android Room DAO implementations, Android-specific APIs
├── iosMain/         : iOS CoreData/platform implementations
└── commonTest/      : shared unit tests

:androidApp          (Android application, depends on :shared)
:iosApp              (Xcode project, depends on :shared via framework export)
:desktopApp          (JVM application, depends on :shared)
```

WHY: Business logic changes once, ships everywhere. Platform modules implement the platform contract — they do not contain business logic. If a use case is duplicated across platform modules, it belongs in `commonMain`.

### `expect` / `actual`

Declare platform abstractions in `commonMain`, implement in each target:

```kotlin
// commonMain
expect class PlatformContext

expect fun createDatabaseDriver(context: PlatformContext): SqlDriver

// androidMain
actual typealias PlatformContext = Context

actual fun createDatabaseDriver(context: PlatformContext): SqlDriver {
    return AndroidSqliteDriver(AppDatabase.Schema, context, "app.db")
}

// iosMain
actual class PlatformContext  // no-arg on iOS

actual fun createDatabaseDriver(context: PlatformContext): SqlDriver {
    return NativeSqliteDriver(AppDatabase.Schema, "app.db")
}
```

### Rules

- `expect`/`actual` for platform APIs only. If the implementation is identical across all targets, it belongs in `commonMain` without `expect`/`actual`.
- Prefer interfaces over `expect`/`actual` when the abstraction is DI-injected. `expect`/`actual` is for types and functions that must resolve at compile time. Interfaces are for dependencies that can be swapped at runtime (testing, configuration).
- Never put `expect`/`actual` on domain models. Domain models are platform-agnostic by definition.

WHY (interfaces over expect/actual for DI): `expect`/`actual` resolves at compile time per target — you cannot swap implementations in tests without a separate test source set. An interface injected via Koin or a manual DI graph can be faked trivially.

### Dependency injection in KMP

Hilt is Android-only (Dagger is JVM-only). For KMP projects, use Koin:

```kotlin
// commonMain
val sharedModule = module {
    singleOf(::AlbumRepository)
    factoryOf(::GetAlbumsUseCase)
    viewModelOf(::AlbumViewModel)
}

// androidMain
val androidModule = module {
    single<SqlDriver> { AndroidSqliteDriver(AppDatabase.Schema, get(), "app.db") }
}

// iosMain
val iosModule = module {
    single<SqlDriver> { NativeSqliteDriver(AppDatabase.Schema, "app.db") }
}
```

WHY: Koin is pure Kotlin with no annotation processing — it works identically on all KMP targets. Hilt generates Android-specific components via KSP that cannot run on iOS or desktop.

### Networking

Ktor client for all HTTP in KMP. Platform engines:

```kotlin
// commonMain
val httpClient = HttpClient {
    install(ContentNegotiation) { json(Json { ignoreUnknownKeys = true }) }
    install(Logging) { level = LogLevel.INFO }
}

// androidMain — OkHttp engine
actual fun createEngine(): HttpClientEngine = OkHttp.create()

// iosMain — Darwin engine
actual fun createEngine(): HttpClientEngine = Darwin.create()
```

WHY: Ktor's multiplatform client uses the platform's native HTTP stack. OkHttp on Android, NSURLSession on iOS. No bridging overhead, platform-appropriate TLS and caching.

### Database

SQLDelight for KMP projects (Room is Android-only). Generates type-safe Kotlin from SQL:

```sql
-- Album.sq
selectAll:
SELECT id, title, artist FROM album ORDER BY title;

insertAlbum:
INSERT INTO album (title, artist) VALUES (?, ?);
```

```kotlin
// commonMain — generated API
val albums: Flow<List<Album>> = queries.selectAll().asFlow().mapToList(Dispatchers.IO)
```

WHY: Room generates Android-specific code (SupportSQLiteDatabase). SQLDelight generates pure Kotlin from SQL and supports Android, iOS, JVM, and JS/Wasm targets with platform-native drivers.

### Testing in KMP

- `commonTest` for platform-agnostic tests (business logic, use cases, ViewModels with faked repositories)
- Platform test source sets (`androidTest`, `iosTest`) only for code that uses `actual` implementations
- Use `kotlinx-coroutines-test` with `runTest` in `commonTest` — it works on all targets
- Fakes over mocks in `commonTest`: MockK is JVM-only

WHY: Tests in `commonTest` run on every target in CI, catching platform-specific issues (number formatting, string normalization, date handling) that single-platform tests miss.

---

## Anti-Patterns

1. **`LiveData` in new code**: use `StateFlow`
2. **`GlobalScope`**: use structured concurrency (`viewModelScope`, `lifecycleScope`)
3. **`!!` (non-null assertion)**: use safe calls, `requireNotNull()`, or restructure
4. **`fallbackToDestructiveMigration`**: write explicit Room migrations
5. **Stateful composables**: hoist state to ViewModel
6. **Manual DI**: use Hilt, not manual factory patterns
7. **Blocking on `Dispatchers.Main`**: switch to `IO` for I/O work
8. **Bare `catch (Exception)`**: see STANDARDS.md § No Silent Catch
9. **Mutable state exposed directly**: private `MutableStateFlow`, public `StateFlow`
10. **Missing `@Preview`**: every reusable composable gets a preview
11. **kapt for annotation processing**: use KSP (kapt is maintenance-only)
12. **Gson in new code**: use kotlinx.serialization (or Moshi for existing JVM-only projects)
13. **String-based navigation routes**: use `@Serializable` route classes (Navigation 2.8+)
14. **NavController in ViewModel**: emit navigation events via `Channel`, let the UI navigate
15. **Cold flows from ViewModel**: use `stateIn`/`shareIn` to share upstream; cold flows restart per collector
16. **Nested `collect` calls**: compose flows with `flatMapLatest`, `combine`, or `zip`
17. **`expect`/`actual` on domain models**: domain models are platform-agnostic, keep them in `commonMain`
18. **Hilt in KMP**: Hilt is Android-only; use Koin for multiplatform DI
19. **Room in KMP**: Room is Android-only; use SQLDelight for multiplatform persistence
20. **`androidx.compose.*` imports in `commonMain`**: use Compose Multiplatform imports for cross-platform builds (Material 3 is available via CMP)
21. **MockK in `commonTest`**: MockK is JVM-only; use fakes in shared test source sets
