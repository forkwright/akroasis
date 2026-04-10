# Python

> Additive to STANDARDS.md. Read that first. Everything here is Python-specific.
>
> **Key decisions:** 3.13+, uv packages, ruff lint/format, mypy strict, anyio async, polars data, msgspec serialization, pydantic at boundaries, loguru logging, contextvars propagation, py-spy profiling, src/ layout.

---

## Toolchain

- **Version:** 3.13+ (latest stable)
- **Package manager:** `uv` (replaces pip, pip-tools, venv, pyenv, pipx)
- **Linter:** `ruff` (replaces flake8, isort, pyupgrade, bandit subset)
- **Formatter:** `ruff format` (replaces black)
- **Type checker:** `mypy --strict` or `pyright`
- **Config:** `pyproject.toml` (single config file: no `setup.cfg`, `.flake8`, etc.)
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
- No `requirements.txt` in new projects: use `uv.lock`

### ruff configuration

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

Do not use `select = ["ALL"]`: it enables unstable/preview rules and creates churn on upgrades.

---

## Naming

See STANDARDS.md § Naming for universal conventions.

| Element | Convention | Example |
|---------|-----------|---------|
| Files / Modules | `snake_case.py` | `session_store.py` |
| Private | `_leading_underscore` | `_internal_state`, `_parse_raw` |

---

## Type system

### Type hints on everything

All function signatures get type hints. No exceptions. Return types included.

```python
def load_config(path: Path) -> Config:
    ...

def process_batch(items: list[str], *, timeout: float = 30.0) -> dict[str, int]:
    ...
```

### Modern type syntax

- `list[str]` not `List[str]` (3.9+)
- `dict[str, int]` not `Dict[str, int]`
- `str | None` not `Optional[str]` (3.10+)
- `type` statement for type aliases (3.12+): `type Vector = list[float]`
- `@override` on subclass methods (3.12+): catches stale overrides when parent methods change
- `TypeIs` over `TypeGuard` for type narrowing (3.13+): narrows both branches, not just the truthy one
- `warnings.deprecated()` decorator (3.13+): visible to type checkers and runtime
- `match` statements for structural pattern matching (3.10+): use for discriminated unions, command dispatch, and multi-field destructuring

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

### Structural pattern matching

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

Don't use `match` for value comparisons where `if`/`elif` is clearer.

### Dataclasses for internal data

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
- Use `@dataclass` for internal structured data: no validation overhead

### Pydantic v2 at boundaries

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

### `Path` objects over strings

```python
from pathlib import Path

config_path = Path("/etc/app/config.yaml")
output_dir = config_path.parent / "output"
```

Never concatenate paths with string operations or `os.path.join`.

---

## Error handling

See STANDARDS.md § Error Handling for universal principles.

### Custom exception hierarchies

```python
class AppError(Exception):
    """Base for all application errors."""

class ConfigError(AppError):
    """Configuration loading or parsing failure."""

class SessionError(AppError):
    """Session lifecycle failure."""
```

### ExceptionGroup and `except*`

Required knowledge when using `TaskGroup` or `anyio` task groups: they raise `ExceptionGroup` when multiple tasks fail:

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

### CLI tools

- `sys.exit(1)` for fatal errors
- Error messages to stderr: `print("error: ...", file=sys.stderr)`
- No `exec()` or `eval()`

### Context managers

Use `with` for all resource management. Never manually `open()`/`close()`.

```python
with open(path) as f:
    data = f.read()
```

---

## Async & concurrency

### `anyio` for async i/O

`anyio` over raw `asyncio`: backend-agnostic structured concurrency.

- `anyio.create_task_group()` for structured concurrency
- `anyio.to_thread.run_sync()` for blocking calls in async context
- `anyio.from_thread.run()` for calling async from sync
- Never `asyncio.run()` inside an already-running loop

When `anyio` is not available, use `asyncio.TaskGroup` (3.11+): no bare `create_task` without tracking.

### No global state mutation

Each function/cell should be independently re-runnable. No reliance on execution order through mutable globals.

### Vectorized over loops (Data work)

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

## Profiling

See PERFORMANCE.md for universal measurement principles. Profile before optimizing. These are the Python-specific tools.

### CPU profiling

Use `cProfile` for quick hot-path identification. Use `py-spy` for production profiling without code changes.

```bash
# cProfile: built-in, zero dependencies
python -m cProfile -s cumulative my_script.py

# py-spy: sampling profiler, attaches to running process
py-spy top --pid 12345
py-spy record -o profile.svg -- python my_script.py
```

- `py-spy` is preferred for services: no instrumentation overhead, produces flame graphs directly
- `cProfile` is fine for scripts and one-off investigation
- Never leave profiling hooks in production code paths

### Memory profiling

Use `memray` for allocation tracking. It traces every allocation with negligible overhead compared to `tracemalloc`.

```bash
# memray: trace allocations, generate flame graph
memray run my_script.py
memray flamegraph memray-my_script.py.bin

# memray: attach to running process
memray attach 12345
```

- `memray` over `tracemalloc`: richer output (flame graphs, leak detection), lower overhead
- For quick checks, `tracemalloc` is acceptable (stdlib, no install)
- Track peak memory in benchmarks for data-heavy code (polars pipelines, large msgspec payloads)

### Line-level profiling

Use `scalene` when you need CPU, memory, and GPU profiling at line granularity.

```bash
scalene my_script.py
```

- WHY: `cProfile` shows function-level time. `scalene` shows which *line* is slow and whether the bottleneck is CPU, memory allocation, or copying.
- Use for investigating specific functions after `py-spy` identifies the hot path

### Async profiling

For `anyio`/`asyncio` workloads, use `py-spy` (captures native and Python frames across coroutines) or `yappi` with async clock mode.

```python
import yappi

yappi.set_clock_type('wall')  # wall clock, not CPU: async I/O shows up
yappi.start()
# ... run async code ...
yappi.stop()
yappi.get_func_stats().print_all()
```

- WHY: `cProfile` measures CPU time per call. Async code spends time waiting, not computing. Wall-clock profiling shows where coroutines actually block.

### Benchmarking

Use `pytest-benchmark` for repeatable microbenchmarks. Benchmarks live in `benches/` or `tests/bench_*.py`.

```python
def test_parse_config(benchmark: BenchmarkFixture) -> None:
    data = load_fixture('config.toml')
    result = benchmark(parse_config, data)
    assert result.host == 'localhost'
```

- Commit benchmarks alongside the optimization
- Track results over time: a 20%+ regression without a feature justification is a bug

---

## Context propagation

### `contextvars` for request-scoped state

Use `contextvars.ContextVar` for propagating request IDs, correlation IDs, and trace context through async call stacks. Never use thread-locals in async code. Never pass context through function parameters when the context is cross-cutting.

```python
import contextvars

request_id: contextvars.ContextVar[str] = contextvars.ContextVar('request_id')
```

- WHY: `threading.local()` breaks in async code because multiple coroutines share a thread. `contextvars` is the stdlib solution: each task gets its own context automatically.

### Setting context at boundaries

Set context vars at the entry point (HTTP handler, CLI command, message consumer). All downstream code reads from the context var without explicit parameter passing.

```python
import contextvars
from uuid import uuid4

correlation_id: contextvars.ContextVar[str] = contextvars.ContextVar('correlation_id')

async def handle_request(request: Request) -> Response:
    token = correlation_id.set(request.headers.get('x-correlation-id', str(uuid4())))
    try:
        return await process(request)
    finally:
        correlation_id.reset(token)
```

- Always `reset()` in a `finally` block. WHY: prevents context leaking between requests when tasks are reused.

### Structured logging with context

Integrate `contextvars` with `loguru` so every log line automatically includes the correlation ID.

```python
from loguru import logger

def correlation_filter(record: dict) -> bool:
    record['extra']['correlation_id'] = correlation_id.get('')
    return True

logger.configure(
    patcher=lambda record: record['extra'].update(
        correlation_id=correlation_id.get('none')
    ),
)

# Every log line now includes correlation_id without explicit passing
logger.info('processing started')
```

- WHY: manual `logger.bind(correlation_id=...)` at every call site is error-prone and noisy. Centralize once, emit everywhere.

### Task group context propagation

`anyio` and `asyncio.TaskGroup` automatically copy the current context to child tasks. Verify this in your framework; some third-party task spawners do not.

```python
import anyio

async def parent() -> None:
    correlation_id.set('abc-123')
    async with anyio.create_task_group() as tg:
        tg.start_soon(child)  # child sees correlation_id='abc-123'

async def child() -> None:
    cid = correlation_id.get()  # 'abc-123'
    logger.info(f'child running with {cid}')
```

- If using raw `loop.create_task()`, context is copied automatically (Python 3.12+)
- If using `loop.run_in_executor()`, context is NOT propagated. Use `contextvars.copy_context().run()` explicitly.

### Cross-process context

For distributed traces (HTTP calls, message queues), serialize context into headers. Use W3C Trace Context (`traceparent`/`tracestate`) or a custom `x-correlation-id` header.

```python
import httpx

async def call_downstream(url: str) -> httpx.Response:
    headers = {'x-correlation-id': correlation_id.get('none')}
    async with httpx.AsyncClient() as client:
        return await client.get(url, headers=headers)
```

- WHY: context vars are process-local. Distributed tracing requires explicit serialization at process boundaries.
- If using OpenTelemetry, its propagators handle this automatically. Otherwise, propagate manually.

---

## Serialization

### msgspec for high-Throughput paths

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

## Package organization

### `src/` layout

Use `src/` layout for all packages. WHY: prevents accidental import of the development directory over the installed package. `import my_package` always resolves to the installed version, not the local source tree.

```
project/
├── src/
│   └── my_package/
│       ├── __init__.py
│       ├── _internal/
│       │   ├── __init__.py
│       │   └── parser.py
│       ├── cli.py
│       ├── config.py
│       ├── core.py
│       └── py.typed
├── tests/
│   ├── conftest.py
│   ├── test_core.py
│   └── test_config.py
├── benches/
│   └── bench_parser.py
├── pyproject.toml
└── uv.lock
```

### Explicit packages with `__init__.py`

Every package directory gets an `__init__.py`. No namespace packages in application code. WHY: namespace packages have implicit resolution rules that cause confusing import failures and make tooling (mypy, pytest discovery) less reliable.

### `__all__` defines the public API

Every `__init__.py` must define `__all__`. This is the module's public contract.

```python
# src/my_package/__init__.py
__all__ = ['Config', 'Session', 'load_config']

from .config import Config, load_config
from .core import Session
```

- WHY: without `__all__`, `from my_package import *` exports everything, including internal helpers. `__all__` makes the public API explicit and reviewable.
- Anything not in `__all__` is internal. Downstream code importing it takes on breakage risk.

### Internal modules

Prefix internal modules and packages with `_underscore`. Group private implementation details in a `_internal/` subpackage when the private surface is large.

```python
# Public API: importable by users
from my_package import Config, load_config

# Internal: not part of the contract, may change without notice
from my_package._internal.parser import RawConfigParser  # at caller's risk
```

- WHY: Python has no access modifiers. The `_prefix` convention is the only signal to downstream code that a name is not part of the public API.

### `py.typed` marker

Include an empty `py.typed` file in the package root. WHY: tells mypy and pyright that this package ships inline type hints. Without it, type checkers treat the package as untyped and skip checking call sites.

### Module size

Split modules when they exceed ~300 lines. A module with 800 lines of mixed concerns is hard to navigate and harder to test in isolation. Group by domain concept, not by implementation pattern (don't create `utils.py`, `helpers.py`, or `misc.py`).

- WHY: a `utils.py` file is a gravity well. Every unrelated function lands there. It grows unbounded, has no cohesion, and becomes a dependency bottleneck that everything imports.

### Circular imports

Circular imports are a design signal: the dependency graph is tangled. Fix by extracting the shared dependency into a lower-level module.

```python
# Wrong: a.py imports b.py, b.py imports a.py
# Symptom: ImportError at runtime, or type-checking failures

# Fix: extract the shared type/function into a third module
# shared.py defines the type, both a.py and b.py import from shared.py
```

- Use `TYPE_CHECKING` blocks only as a last resort for circular type references, not as a general fix for circular runtime imports.
- WHY: `TYPE_CHECKING` hides the cycle from runtime but doesn't fix the dependency tangle. The modules are still coupled.

### Entry points

CLI entry points use `pyproject.toml` `[project.scripts]`, not `if __name__ == '__main__'` in library modules.

```toml
[project.scripts]
my-tool = "my_package.cli:main"
```

- WHY: `[project.scripts]` creates proper console scripts on install. `__main__.py` is acceptable for `python -m my_package` invocation but should delegate to the same entry point function.

---

## Testing

See TESTING.md for all testing principles (naming, isolation, coverage, test data, property testing).

Python-specific framework choices:

- **Framework:** `pytest`
- **Fixtures:** `@pytest.fixture` for setup, not `setUp()` methods
- **Parametrize:** `@pytest.mark.parametrize` for testing multiple inputs
- **Property testing:** `hypothesis`
- **Mocking:** `unittest.mock.patch` at module boundaries, not on internals
- **No `print` debugging in tests.** Use `pytest -s` and `logging` if needed.
- **Project layout:** see Package organization above for `src/` layout, test directory structure

---

## Dependencies

**Preferred:**
- `uv` (package management), `ruff` (lint + format), `pytest` (testing)
- `polars` (data), `msgspec` (serialization), `pydantic` (validation at boundaries)
- `anyio` (async), `aiohttp` / `httpx` (HTTP client)
- `typer` (CLI), `loguru` (logging)

**Banned:**
- `os.path` for path manipulation: use `pathlib.Path`
- `format()` / `%` string formatting: use f-strings
- `exec()` / `eval()`: security risk, always avoidable
- `type: ignore` without explanation: fix the type error
- `Optional[str]`: use `str | None`
- `List[str]`, `Dict[str, int]`: use built-in generics
- `TypeGuard` for narrowing: use `TypeIs` (3.13+)
- `pip` / `pip-tools`: use `uv`
- `pandas` in new code: use `polars` (pandas acceptable in existing codebases)

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

### String formatting

f-strings always. Single quotes for strings.

```python
name = 'world'
message = f'hello, {name}'
```

Nested f-strings are valid (3.12+): no need for temp variables or `str.format()` workarounds.

### Comprehensions over map/Filter

```python
# Preferred
squares = [x**2 for x in range(10) if x % 2 == 0]

# Not preferred
squares = list(map(lambda x: x**2, filter(lambda x: x % 2 == 0, range(10))))
```

---

## Anti-Patterns

AI agents consistently produce these in Python:

1. **Missing type hints**: every function signature, no exceptions
2. **`Optional[str]` instead of `str | None`**: use modern union syntax
3. **`List[str]` instead of `list[str]`**: use built-in generics (3.9+)
4. **Bare `except Exception`**: see STANDARDS.md § No Silent Catch
5. **String path concatenation**: use `pathlib.Path`
6. **`os.path.join` over `/` operator**: `Path("a") / "b"` is cleaner
7. **Print for debugging**: use `loguru` or structured logging
8. **Mutable default arguments**: `def f(items: list[str] = [])` is a classic bug
9. **`import *`**: explicit imports only
10. **Ignoring `__all__`**: define public API explicitly in modules
11. **`pip install` in projects**: use `uv add` / `uv sync`
12. **`pandas` in new code**: use `polars` for data processing
13. **Missing `@override`**: use on all subclass method overrides (3.12+)
14. **`if`/`elif` chains for structured dispatch**: use `match` for 3+ branches on type or discriminant field (3.10+)
15. **Flat layout without `src/`**: use `src/` layout to prevent import confusion
16. **Missing `__init__.py`**: explicit packages, not namespace packages. Define `__all__` for public API.
17. **`threading.local()` in async code**: use `contextvars.ContextVar`. Thread-locals break when multiple coroutines share a thread.
18. **Passing context through every function signature**: use `contextvars` for cross-cutting concerns (request IDs, correlation IDs, trace context)
19. **`utils.py` / `helpers.py` catch-all modules**: group by domain concept, not by "miscellaneous". These files grow unbounded and have no cohesion.
20. **Circular imports patched with `TYPE_CHECKING`**: fix the dependency graph. Extract shared types into a lower-level module.
21. **Optimizing without profiling**: use `py-spy` or `cProfile` first. Guessing at bottlenecks wastes time and often makes things worse.
22. **Missing `py.typed` marker**: include in package root so type checkers recognize inline hints
