# Kotlin

> Additive to STANDARDS.md. Read that first. Everything here is Kotlin-specific.
>
> Target: Kotlin 2.x (K2 compiler), Android (Jetpack Compose, Hilt, Room). Akroasis media player.
>
> **Key decisions:** K2 compiler, Compose + Material 3, Hilt DI, Room DB, KSP (not kapt), kotlinx.serialization, StateFlow (not LiveData), coroutines.

---

## Toolchain

- **Language:** Kotlin 2.x (K2 compiler, default since 2.0)
- **UI:** Jetpack Compose with Material 3
- **DI:** Hilt
- **Database:** Room
- **Annotation processing:** KSP (not kapt — kapt is maintenance-only)
- **Serialization:** kotlinx.serialization
- **Build/validate:**
  ```bash
  cd android && ./gradlew build && ./gradlew test
  ```

---

## Naming

| Element | Convention | Example |
|---------|-----------|---------|
| Files | `PascalCase.kt` | `PlayerViewModel.kt` |
| Classes / Interfaces | `PascalCase` | `PlayerViewModel`, `PlaybackRepository` |
| Functions / Properties | `camelCase` | `loadAlbum`, `isPlaying` |
| Constants | `UPPER_SNAKE_CASE` or `PascalCase` | `MAX_RETRY_COUNT`, `DefaultTimeout` |
| Packages | `lowercase` | `app.akroasis.ui.player` |
| Composables | `PascalCase` (Compose convention) | `TrackItem`, `PlayerScreen` |
| StateFlows | `_camelCase` (private) / `camelCase` (public) | `_uiState` / `uiState` |

---

## Type System

### Sealed Hierarchies for State

```kotlin
sealed interface PlayerUiState {
    data object Loading : PlayerUiState
    data class Playing(val track: Track, val progress: Float) : PlayerUiState
    data class Error(val message: String) : PlayerUiState
}
```

Exhaustive `when` expressions over sealed types. The compiler enforces all branches.

`data object` for sealed hierarchy leaves that carry no data — provides proper `toString()`, `equals()`, `hashCode()`.

### Data Classes for Value Types

```kotlin
data class Track(
    val id: Long,
    val title: String,
    val duration: Duration,
    val albumId: Long,
)
```

### Value Classes

Use `value class` for type-safe wrappers with zero allocation overhead:

```kotlin
@JvmInline
value class TrackId(val value: Long)

@JvmInline
value class AlbumId(val value: Long)
```

Single-property restriction. Boxing occurs when used as a generic type parameter or through an interface. `@JvmInline` required for JVM targets.

### Null Safety

Kotlin's type system distinguishes nullable (`String?`) from non-null (`String`). Use it. Never use `!!` without a clear invariant justification. Prefer `?.let`, `?:`, `requireNotNull()`.

---

## Error Handling

### `Result<T>` for Operations That Can Fail

```kotlin
fun loadAlbum(id: Int): Result<Album> {
    return runCatching {
        repository.getAlbum(id)
    }
}
```

### Sealed Error Hierarchies

```kotlin
sealed class AppError {
    data class NotFound(val id: String) : AppError()
    data class Network(val cause: Throwable) : AppError()
    data class Unauthorized(val reason: String) : AppError()
}
```

### Rules

- Never bare `catch (Exception)` — catch specific types
- `runCatching { }` for concise error handling where appropriate
- Propagate errors to the ViewModel — let the UI layer decide presentation

---

## Async & Concurrency

### Coroutines with Structured Concurrency

- `viewModelScope` for ViewModel-scoped coroutines
- `Dispatchers.IO` for I/O operations, never `Dispatchers.Main` for blocking work
- `withContext()` for dispatcher switching
- **Never `GlobalScope`** — use structured concurrency always

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

### StateFlow for Reactive State

- `MutableStateFlow` private, expose as `StateFlow`
- `stateIn` with `SharingStarted.WhileSubscribed(5000)` for flow collection
- No `LiveData` in new code — `StateFlow` everywhere

```kotlin
private val _uiState = MutableStateFlow<PlayerUiState>(PlayerUiState.Loading)
val uiState: StateFlow<PlayerUiState> = _uiState.asStateFlow()
```

---

## Compose

### State Hoisting

Composables are stateless by default. State lives in the ViewModel. Pass state down, emit events up.

```kotlin
@Composable
fun TrackItem(
    track: Track,
    isPlaying: Boolean,
    onTrackClick: (Track) -> Unit,
    modifier: Modifier = Modifier,
) {
    // stateless — all state passed in
}
```

### Rules

- `remember` / `rememberSaveable` for local ephemeral state only
- `@Preview` on all reusable composables
- Material 3 design system
- Extract reusable composables into separate files

### Compose Multiplatform

iOS: stable (since CMP 1.8). Desktop: stable. Web (Wasm): beta. Use for shared UI across Android/iOS when applicable.

---

## Dependency Injection

**Hilt** for all DI.

- `@HiltViewModel` on all ViewModels
- `@Inject constructor` — no manual instantiation
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

**kotlinx.serialization** for all new code. Compiler plugin — no reflection.

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
- **Gson is legacy** — does not understand Kotlin nullability, default parameters, or value classes. Migrate away.
- Moshi acceptable for existing Android/JVM-only projects

---

## Testing

- **Framework:** JUnit 5 + Turbine (for Flow testing)
- **Names:** `loadAlbum_emitsLoaded_whenSuccess`, not `test1`
- **Coroutine testing:** `runTest { }` with `TestDispatcher`
- **Compose testing:** `createComposeRule()` for UI tests
- **Mocking:** MockK at interface boundaries

---

## Architecture

MVVM with unidirectional data flow:

```
app/src/main/java/app/akroasis/
├── data/           — repositories, data sources, Room DAOs
├── domain/         — use cases, domain models
├── ui/             — screens, composables, ViewModels
├── di/             — Hilt modules
└── util/           — shared utilities
```

- ViewModel exposes `StateFlow<UiState>`, never `LiveData`
- UI observes state, emits events
- Repository is the single source of truth for data
- Use cases encapsulate business logic between ViewModel and Repository

---

## Anti-Patterns

1. **`LiveData` in new code** — use `StateFlow`
2. **`GlobalScope`** — use structured concurrency (`viewModelScope`, `lifecycleScope`)
3. **`!!` (non-null assertion)** — use safe calls, `requireNotNull()`, or restructure
4. **`fallbackToDestructiveMigration`** — write explicit Room migrations
5. **Stateful composables** — hoist state to ViewModel
6. **Manual DI** — use Hilt, not manual factory patterns
7. **Blocking on `Dispatchers.Main`** — switch to `IO` for I/O work
8. **Bare `catch (Exception)`** — catch specific types
9. **Mutable state exposed directly** — private `MutableStateFlow`, public `StateFlow`
10. **Missing `@Preview`** — every reusable composable gets a preview
11. **kapt for annotation processing** — use KSP (kapt is maintenance-only)
12. **Gson in new code** — use kotlinx.serialization (or Moshi for existing JVM-only projects)
