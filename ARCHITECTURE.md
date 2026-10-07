# Architecture

This document describes how the Laravel framework is expressed in Rust. It is
the contract every Illuminate component follows. If you are contributing to
the framework, read it first.

## Philosophy

Laravel values **developer happiness**, **expressive and elegant syntax**,
and **convention over configuration**. The Rust port keeps every one of those
values. When designing an API, ask: *"What would this look like in Laravel?"*
and then find the most natural Rust spelling of it.

- Method and type names mirror Laravel, converted to Rust conventions:
  `whereIn` → `where_in`, `firstOrFail` → `first_or_fail`,
  `Str::snake` → `Str::snake`, `Route::get` → `Route::get`.
- `where` is a Rust keyword, so it becomes `where_`. Laravel's optional-arity
  methods are split into distinct names (`where_(col, value)` and
  `where_op(col, op, value)`).
- Doc comments are written in Laravel's voice: friendly, confident, brief.
  Public APIs have doc comments, and most have a runnable example.
- Facades (`Route`, `DB`, `Cache`, `Config`, `Log`, `Hash`, ...) are
  zero-sized structs with associated functions that resolve the underlying
  service from the container.
- Global helpers (`config()`, `view()`, `route()`, `now()`, `collect()`,
  `abort()`, ...) exist as plain functions.

## Workspace layout

```
laravel-rust/
├── src/lib.rs                  # the `laravel` crate: prelude, facades, re-exports
├── illuminate/<component>/     # one crate per Illuminate component
│   └── illuminate-<component>
├── packages/<package>/         # first-party packages (laravel/sanctum, ...)
│   └── laravel-<package>
├── installer/                  # `laravel-rust new` (laravel/installer), embeds the skeleton
└── skeleton/                   # the application skeleton (laravel/laravel)
```

| Crate | Laravel component |
| --- | --- |
| `illuminate-support` | Support + Collections + Conditionable (Str, Arr, Collection, Carbon, helpers) |
| `illuminate-container` | Container (+ the `ServiceProvider` contract) |
| `illuminate-config` | Config |
| `illuminate-http` | Http (Request, Response, middleware contract, server) |
| `illuminate-pipeline` | Pipeline |
| `illuminate-routing` | Routing (Router, URL generator, redirects, controllers) |
| `illuminate-view` | View (Blade) |
| `illuminate-validation` | Validation |
| `illuminate-database` | Database (query builder, schema, migrations, Eloquent) |
| `illuminate-macros` | Procedural macros (`#[derive(Model)]`, ...) |
| `illuminate-pagination` | Pagination |
| `illuminate-encryption` / `-hashing` | Encryption / Hashing |
| `illuminate-cookie` / `-session` | Cookie / Session |
| `illuminate-cache` | Cache (+ RateLimiter) |
| `illuminate-redis` | Redis |
| `illuminate-events` / `-log` / `-filesystem` / `-translation` | Events / Log / Filesystem / Translation |
| `illuminate-console` | Console (Artisan commands, scheduling) |
| `illuminate-auth` | Auth (guards, gates, policies) |
| `illuminate-queue` / `-mail` / `-notifications` | Queue + Bus / Mail / Notifications |
| `illuminate-process` / `-concurrency` | Process / Concurrency |
| `illuminate-http-client` | Http Client (the `Http` facade, AWS request signing) |
| `illuminate-http-resources` | Eloquent API resources |
| `illuminate-broadcasting` | Broadcasting |
| `illuminate-image` | Image |
| `illuminate-json-schema` | JsonSchema |
| `illuminate-foundation` | Foundation (Application, kernels, exception handler, Artisan commands, testing) |
| `laravel-build` (`illuminate/build`) | Build-time discovery of migrations, seeders, commands, components, and policies |

Dependencies always point "down" this list: a component may depend on
`support`, `container`, `config` and `http`, but never on `foundation`.
When a component needs something from a crate above it (the scheduler
emailing output, say), it defines a small trait the foundation implements
and binds in the container (`ScheduleOutputMailer`, `EventMutex`, ...).

### Packages

First-party packages live in `packages/` and depend on the components, never
the other way around. A package registers its service provider for
discovery — the Rust analog of Composer's `extra.laravel.providers` — with
`illuminate_container::discover_provider!("laravel-sanctum",
SanctumServiceProvider);`. The `laravel` crate re-exports each package behind
a cargo feature (`features = ["sanctum"]`), and installers like
`install:api` turn the feature on.

Packages ship compiled, so the files they publish with `vendor:publish` are
embedded strings rather than paths into the package. A provider registers
them in `boot`, the way Laravel's `$this->publishes()` does:

```rust
self.publishes(app, [Publishable::config("sanctum.rs", CONFIG_STUB)], "sanctum-config");
```

Destinations are relative to the application's directories (`config/`,
`database/migrations/`, `lang/`, ...), resolved when the command runs.

## Core conventions

### Dynamic values

PHP arrays become [`illuminate_support::Value`] (a re-export of
`serde_json::Value` with insertion-ordered objects). Request input,
configuration, view data and raw database rows are all `Value`s.

- `ValueExt` adds PHP semantics: `truthy()`, `is_blank()`,
  `to_string_lossy()`, `to_i64_lossy()`, and dot-notation access via
  `dot("a.b.c")`.
- `cast::<T>(value)` converts a `Value` into a concrete type *leniently*
  (`"1"` → `true`, `"42"` → `42`, `1` → `true`).
- `json!` builds values inline.

### Errors are exceptions

Every fallible API returns `illuminate_support::Result<T>` (an
`anyhow`-style error with downcasting). Laravel's exceptions are concrete
error types (`HttpException`, `ValidationException`, `QueryException`,
`ModelNotFoundException`, `AuthenticationException`, ...). The exception
handler downcasts to decide how to report and render each one.

`abort(404)` returns `Err(HttpException)` so it composes with `?`:

```rust
abort_if(!user.is_admin, 403)?;
let podcast = Podcast::find(id).await?.ok_or_else(|| HttpException::new(404))?;
```

### The container

`illuminate_container::Container` binds services by **type**
(`bind`, `singleton`, `scoped`, `instance`) and resolves them as `Arc<T>`
(`make`, `try_make`). Trait objects work: `container.singleton::<dyn Store>(...)`.

- `Container::get_instance()` returns the current container: a thread-local
  instance (set with `Container::set_local_instance`, used by tests) wins over
  the global one (set with `Container::set_instance`).
- `app::<T>()` / `try_app::<T>()` resolve from the current container.
- `ServiceProvider` has `register(&Container)` and `boot(&Container)`.
  Every component exposes a provider that binds its manager/services,
  reading configuration lazily from the `illuminate_config::Repository`
  registered in the container.

Facades resolve through `app::<T>()`. A facade must not cache the service
in a static: tests swap services by binding new instances.

### Configuration

Configuration is a `Value` tree in `illuminate_config::Repository`,
accessed with dot notation: `config("database.default")`,
`Config::string("app.name")`. Components never read `.env` directly; they
read config. File system paths in config are absolute (the skeleton's config
files use `storage_path()`, `resource_path()`, ...).

### Async

The framework is async on Tokio. Use `async fn` in inherent methods, and the
`async_trait` attribute (re-exported by `illuminate_http::async_trait`) for
object-safe traits (`Middleware`, cache stores, commands, jobs, ...). Futures
handed to the framework must be `Send + 'static`.

### HTTP

- `Request` is a cheap, cloneable **shared handle** with interior mutability
  (`merge`, `set_route`, extensions). Components attach per-request state
  through typed extensions: `request.set_extension(Arc<Store>)` /
  `request.extension::<Store>()`.
- Per-request APIs that live in higher-level crates are added to `Request`
  with *extension traits* exported from each crate's prelude
  (`request.session()`, `request.user()`, `request.validate(...)`).
- While a request is handled, it is the *current request*
  (`illuminate_http::current_request()`), a Tokio task-local. This powers
  helpers like `session()`, `old()`, `auth()` and `request()`.
- `Response` is a single type for plain, JSON, redirect, download and
  streamed responses. Redirects carry flash data (`with`, `with_errors`,
  `with_input`) which the session middleware writes to the session.
- `IntoResponse` converts handler return values. `Result<T, E>` renders its
  error through the bound `dyn ExceptionHandler` (`render_exception`).
- `Middleware` (`async fn handle(&self, request, next) -> Result<Response>`)
  and `Next` live in `illuminate-http` so any component can provide
  middleware without depending on routing. Errors returned by middleware are
  rendered at that layer, so outer middleware always see a response.

### Testing

Each crate has unit tests next to the code and doc-tested examples. Tests
that need the container create their own and install it with
`Container::set_local_instance(...)` so they can run in parallel. Prefer
`#[tokio::test]` (current-thread runtime) for async tests.
