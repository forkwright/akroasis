# Python

> Additive to STANDARDS.md. Read that first. Everything here is Python-specific.
>
> **Key decisions:** 3.13+, uv packages, ruff lint/format, mypy strict, anyio async, polars data, msgspec serialization, pydantic at boundaries, loguru logging.

---

## Toolchain

- **Version:** 3.13+ (latest stable)
- **Package manager:** `uv` (replaces pip, pip-tools, venv, pyenv, pipx)
- **Linter:** `ruff` (replaces flake8, isort, pyupgrade, bandit subset)
- **Formatter:** `ruff format` (replaces black)
- **Type checker:** `mypy --strict` or `pyright`
- **Config:** `pyproject.toml` (single config file — no `setup.cfg`, `.flake8`, etc.)
- **Line length:** 100 characters
- **Build/validate:**
  ```bash
  ruff check .
  ruff format --check .
  mypy .
  pytest
  ```

### uv

`uv` is the standard for all package management. Rust-based, 10-100x faster than pip.

```bash
uv init                    # new project
uv add polars              # add dependency
uv sync                    # install from lockfile
uv run pytest              # run in managed env
uv python install 3.13     # install Python version
```

- `uv.lock` for reproducible installs (applications)
- `pyproject.toml` `[project.dependencies]` for libraries
- No `requirements.txt` in new projects — use `uv.lock`

### ruff Configuration

```toml
[tool.ruff]
target-version = "py313"
line-length = 100

[tool.ruff.lint]
select = [
    "F",      # pyflakes
    "E", "W", # pycodestyle
    "I",      # isort
    "N",      # pep8-naming
    "UP",     # pyupgrade (modernize syntax for target version)
    "B",      # flake8-bugbear
    "SIM",    # flake8-simplify
    "PTH",    # flake8-use-pathlib
    "C4",     # flake8-comprehensions
    "RET",    # flake8-return
    "TC",     # flake8-type-checking (TYPE_CHECKING blocks)
    "RUF",    # ruff-specific rules
]

[tool.ruff.lint.isort]
known-first-party = ["your_package"]
```

Do not use `select = ["ALL"]` — it enables unstable/preview rules and creates churn on upgrades.

---

## Naming

| Element | Convention | Example |
|---------|-----------|---------|
| Files / Modules | `snake_case.py` | `session_store.py` |
| Classes | `PascalCase` | `SessionStore`, `PromptRenderer` |
| Functions / Methods | `snake_case` | `load_config`, `parse_input` |
| Variables | `snake_case` | `active_count`, `session_id` |
| Constants | `UPPER_SNAKE_CASE` | `MAX_RETRIES`, `DEFAULT_TIMEOUT` |
| Private | `_leading_underscore` | `_internal_state`, `_parse_raw` |

---

## Type System

### Type Hints on Everything

All function signatures get type hints. No exceptions. Return types included.

```python
def load_config(path: Path) -> Config:
    ...

def process_batch(items: list[str], *, timeout: float = 30.0) -> dict[str, int]:
    ...
```

### Modern Type Syntax

- `list[str]` not `List[str]` (3.9+)
- `dict[str, int]` not `Dict[str, int]`
- `str | None` not `Optional[str]` (3.10+)
- `type` statement for type aliases (3.12+): `type Vector = list[float]`
- `@override` on subclass methods (3.12+) — catches stale overrides when parent methods change
- `TypeIs` over `TypeGuard` for type narrowing (3.13+) — narrows both branches, not just the truthy one
- `warnings.deprecated()` decorator (3.13+) — visible to type checkers and runtime
- `match` statements for structural pattern matching (3.10+) — use for discriminated unions, command dispatch, and multi-field destructuring

```python
from typing import override, TypeIs
import warnings

class MyParser(BaseParser):
    @override
    def parse(self, raw: bytes) -> Document:
        ...

def is_valid_session(val: object) -> TypeIs[Session]:
    return isinstance(val, Session)

@warnings.deprecated("use load_config_v2 instead")
def load_config(path: str) -> Config:
    ...
```

### Structural Pattern Matching

Use `match` for multi-branch dispatch on structured data. Replaces `if`/`elif` chains when matching on type, shape, or discriminant fields.

```python
match event:
    case {'type': 'click', 'x': x, 'y': y}:
        handle_click(x, y)
    case {'type': 'key', 'code': code} if code in HOTKEYS:
        handle_hotkey(code)
    case _:
        logger.debug(f'unhandled event: {event}')
```

Use `match` when:
- Branching on a discriminant field (`type`, `kind`, `status`)
- Destructuring nested structures in the same expression
- Three or more branches that would be `isinstance` checks

Don't use `match` for simple value comparisons where `if`/`elif` is clearer.

### Dataclasses for Internal Data

```python
from dataclasses import dataclass

@dataclass(frozen=True, slots=True)
class Config:
    host: str
    port: int
    timeout: float = 30.0
```

- `frozen=True` for immutable value types
- `slots=True` for memory efficiency
- Use `@dataclass` for internal structured data — no validation overhead

### Pydantic v2 at Boundaries

Pydantic for external data (HTTP requests, config files, JSON from APIs). Dataclasses for internal data.

```python
from pydantic import BaseModel

class CreateSessionRequest(BaseModel):
    name: str
    timeout: float = 30.0
    tags: list[str] = []
```

- Pydantic: validation, coercion, error messages, OpenAPI schema generation
- Dataclasses: 6x faster instantiation, no validation overhead
- Rule: Pydantic at the boundary, dataclasses inside

### `Path` Objects Over Strings

```python
from pathlib import Path

config_path = Path("/etc/app/config.yaml")
output_dir = config_path.parent / "output"
```

Never concatenate paths with string operations or `os.path.join`.

---

## Error Handling

### Exceptions, Not Return Codes

```python
# Wrong: sentinel return value
def find_user(name: str) -> User | None:
    ...

# Right: raise on failure, or use a result type
def find_user(name: str) -> User:
    user = db.query(name)
    if user is None:
        raise UserNotFoundError(name)
    return user
```

### Custom Exception Hierarchies

```python
class AppError(Exception):
    """Base for all application errors."""

class ConfigError(AppError):
    """Configuration loading or parsing failure."""

class SessionError(AppError):
    """Session lifecycle failure."""
```

### ExceptionGroup and `except*`

Required knowledge when using `TaskGroup` or `anyio` task groups — they raise `ExceptionGroup` when multiple tasks fail:

```python
try:
    async with anyio.create_task_group() as tg:
        tg.start_soon(fetch_a)
        tg.start_soon(fetch_b)
except* ConnectionError as eg:
    for exc in eg.exceptions:
        logger.error(f"connection failed: {exc}")
except* TimeoutError:
    logger.error("timeout in task group")
```

Don't retrofit `except*` into sequential code where a single `except` suffices.

### CLI Tools

- `sys.exit(1)` for fatal errors
- Error messages to stderr: `print("error: ...", file=sys.stderr)`
- Never bare `except Exception` without logging or re-raising
- No `exec()` or `eval()`

### Context Managers

Use `with` for all resource management. Never manually `open()`/`close()`.

```python
with open(path) as f:
    data = f.read()
```

---

## Async & Concurrency

### `anyio` for Async I/O

`anyio` over raw `asyncio` — backend-agnostic structured concurrency.

- `anyio.create_task_group()` for structured concurrency
- `anyio.to_thread.run_sync()` for blocking calls in async context
- `anyio.from_thread.run()` for calling async from sync
- Never `asyncio.run()` inside an already-running loop

When `anyio` is not available, use `asyncio.TaskGroup` (3.11+) — no bare `create_task` without tracking.

### No Global State Mutation

Each function/cell should be independently re-runnable. No reliance on execution order through mutable globals.

### Vectorized Over Loops (Data Work)

Polars: lazy expressions over row iteration. Use `.lazy()` for query optimization.

```python
import polars as pl

# Polars: lazy, parallel, expressive
result = (
    pl.scan_csv("data.csv")
    .filter(pl.col("status") == "active")
    .group_by("employer_id")
    .agg(pl.col("amount").sum())
    .collect()
)
```

---

## Serialization

### msgspec for High-Throughput Paths

`msgspec` (10-12x faster than Pydantic v2) for internal serialization, message passing, and high-volume data:

```python
import msgspec

class Event(msgspec.Struct, frozen=True):
    id: str
    kind: str
    payload: dict[str, str]

encoder = msgspec.json.Encoder()
decoder = msgspec.json.Decoder(Event)

data = encoder.encode(event)
event = decoder.decode(data)
```

| Use case | Tool |
|----------|------|
| API boundaries, config files | Pydantic v2 |
| Internal data structures | dataclasses |
| High-throughput serialization | msgspec |

---

## Testing

- **Framework:** `pytest`
- **Names:** `test_returns_empty_when_no_items`, not `test_1` or `test_basic`
- **Fixtures:** `@pytest.fixture` for setup, not `setUp()` methods
- **Parametrize:** `@pytest.mark.parametrize` for testing multiple inputs
- **Mocking:** `unittest.mock.patch` at module boundaries, not on internals
- **No `print` debugging in tests.** Use `pytest -s` and `logging` if needed.

### Project Layout

Use `src/` layout for all packages:

```
project/
├── src/
│   └── my_package/
│       ├── __init__.py
│       └── core.py
├── tests/
├── pyproject.toml
└── uv.lock
```

- `__init__.py` in every package directory (explicit packages, not namespace packages)
- Define `__all__` in `__init__.py` to declare the public API
- `src/` layout prevents accidental import of the development directory over the installed package

---

## Dependencies

**Preferred:**
- `uv` (package management), `ruff` (lint + format), `pytest` (testing)
- `polars` (data), `msgspec` (serialization), `pydantic` (validation at boundaries)
- `anyio` (async), `aiohttp` / `httpx` (HTTP client)
- `typer` (CLI), `loguru` (logging)

**Banned:**
- `os.path` for path manipulation — use `pathlib.Path`
- `format()` / `%` string formatting — use f-strings
- `exec()` / `eval()` — security risk, always avoidable
- `type: ignore` without explanation — fix the type error
- `Optional[str]` — use `str | None`
- `List[str]`, `Dict[str, int]` — use built-in generics
- `TypeGuard` for simple narrowing — use `TypeIs` (3.13+)
- `pip` / `pip-tools` — use `uv`
- `pandas` in new code — use `polars` (pandas acceptable in existing codebases)

---

## Style

### Imports

```python
# stdlib
import sys
from pathlib import Path

# third-party
import polars as pl
from loguru import logger

# local
from .config import load_config
```

One blank line between each group. `ruff` enforces sort order.

### String Formatting

f-strings always. Single quotes for strings.

```python
name = 'world'
message = f'hello, {name}'
```

Nested f-strings are valid (3.12+) — no need for temp variables or `str.format()` workarounds.

### Comprehensions Over Map/Filter

```python
# Preferred
squares = [x**2 for x in range(10) if x % 2 == 0]

# Not preferred
squares = list(map(lambda x: x**2, filter(lambda x: x % 2 == 0, range(10))))
```

---

## Anti-Patterns

AI agents consistently produce these in Python:

1. **Missing type hints** — every function signature, no exceptions
2. **`Optional[str]` instead of `str | None`** — use modern union syntax
3. **`List[str]` instead of `list[str]`** — use built-in generics (3.9+)
4. **Bare `except Exception`** — catch specific types, always handle or re-raise
5. **String path concatenation** — use `pathlib.Path`
6. **`os.path.join` over `/` operator** — `Path("a") / "b"` is cleaner
7. **Print for debugging** — use `loguru` or structured logging
8. **Mutable default arguments** — `def f(items: list[str] = [])` is a classic bug
9. **`import *`** — explicit imports only
10. **Ignoring `__all__`** — define public API explicitly in modules
11. **`pip install` in projects** — use `uv add` / `uv sync`
12. **`pandas` in new code** — use `polars` for data processing
13. **Missing `@override`** — use on all subclass method overrides (3.12+)
14. **`if`/`elif` chains for structured dispatch** — use `match` for 3+ branches on type or discriminant field (3.10+)
15. **Flat layout without `src/`** — use `src/` layout to prevent import confusion
16. **Missing `__init__.py`** — explicit packages, not namespace packages. Define `__all__` for public API.
