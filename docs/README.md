# Laravel in Rust

This guide covers what changes when Laravel is written in Rust: how to start
an application, a tour of the main components, and what the port includes.
Everything else works the way the [Laravel documentation](https://laravel.com/docs)
describes it.

```rust
use laravel::prelude::*;

pub fn web() {
    Route::get("/", || async { view("welcome", ()) });

    Route::get("/users/{user}", |user: User| async move { Json(user) });

    Route::post("/podcasts", |request: Request| async move {
        let validated = request
            .validate(rules! {
                "title" => "required|string|max:255",
                "url" => "required|url|unique:podcasts,url",
            })
            .await?;

        let podcast = Podcast::create(validated).await?;

        ProcessPodcast { id: podcast.id }.dispatch().await?;

        Ok::<_, Error>(redirect("/podcasts").with("status", "Podcast queued!"))
    })
    .middleware("auth");
}
```

If you know Laravel, you already know this framework. Method names are the
same in snake case (`whereIn` is `where_in`, `firstOrFail` is
`first_or_fail`), facades are still facades, and helpers are still helpers.
The differences are the ones Rust asks for: handlers are `async`, models are
structs, and the compiler checks your work.

## Getting started

Install the Laravel installer — the equivalent of `laravel/installer` —
then create an application. The command is `laravel-rust`, so it sits
alongside the PHP installer's `laravel` command:

```shell
curl -fsSL https://portsidelabs.io/laravel-rust/install.sh | sh
laravel-rust new example-app
```

The install script builds the installer with Cargo, so you'll need Rust 1.89
or later ([rustup.rs](https://rustup.rs)). It runs `cargo install --git
https://github.com/portside-labs/laravel-rust laravel-installer`, which you
can also run yourself. If you're working on the framework itself, install
from your checkout instead: `cargo install --path installer`.

The installer asks which database you'd like and whether to build your
frontend assets, then copies the application skeleton (the
[`skeleton`](../skeleton) directory, the equivalent of `laravel/laravel`),
writes your `.env` with a fresh application key, builds the application,
and (for SQLite) runs your migrations. New applications depend on the
framework's Git repository, or on the checkout an installer was built from
(or `--path=<dir>`). Then start your application:

```shell
cd example-app
cargo artisan serve
```

`cargo artisan` is a Cargo alias for the application's `artisan` binary, so
every Artisan command works the way you expect: `cargo artisan make:model
Post --all`, `cargo artisan route:list`, `cargo artisan queue:work`, and so
on. Run the test suite with `cargo test`.

Laravel discovers your application's classes at runtime; Rust needs to know
about them when it compiles. The skeleton's build script handles this:
migrations, seeders, Artisan commands, Blade components, and policies placed
in their usual directories are found at build time, with nothing to register
by hand.

## A tour

### Eloquent

Models are plain structs. The derive gives them everything Eloquent has:
conventions for tables and keys, mass assignment, serialization, casts
through field types, soft deletes, scopes, events, relationships, and
factories.

```rust
#[derive(Debug, Clone, Default, Model, Authenticatable)]
#[use_factory(UserFactory)]
#[fillable(name, email, password)]
#[hidden(password, remember_token)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub email: String,
    #[hashed]
    pub password: String,
    pub remember_token: Option<String>,
    #[relation]
    pub posts: Option<Vec<Post>>,
    pub created_at: Option<Carbon>,
    pub updated_at: Option<Carbon>,
}

impl User {
    /// Get the posts written by the user.
    pub fn posts(&self) -> HasMany<Self, Post> {
        self.has_many()
    }
}

let users = User::with("posts")
    .where_("active", true)
    .latest()
    .paginate(15)
    .await?;
```

### API resources

Resources transform models into JSON, with conditional attributes and
paginated responses shaped exactly like Laravel's:

```rust
pub struct UserResource(pub User);

impl JsonResource for UserResource {
    type Model = User;

    fn from_model(user: User) -> Self {
        Self(user)
    }

    fn model(&self) -> &User {
        &self.0
    }

    fn to_array(&self, request: &Request) -> Value {
        json!({
            "id": self.0.id,
            "name": self.0.name,
            "posts": PostResource::collection(self.when_loaded(&self.0.posts)),
        })
    }
}

Route::get("/api/users", || async {
    Ok::<_, Error>(UserResource::collection(User::query().paginate(15).await?))
});
```

### API authentication

`cargo artisan install:api` installs Laravel Sanctum: it turns on the
`laravel` crate's `sanctum` feature, publishes `routes/api.rs` and the
personal access tokens migration, and loads the API routes. Packages are
discovered the way Composer's `extra.laravel.providers` works, so there's
no provider to register:

```rust
use laravel::sanctum::HasApiTokens;

Route::post("/tokens/create", |request: Request| async move {
    let user: User = request.user().unwrap();
    let token = user.create_token(&request.string("token_name"), &["orders:read"]).await?;

    Ok::<_, Error>(Json(json!({"token": token.plain_text_token})))
})
.middleware("auth");

Route::get("/orders", || async { "Orders" })
    .middleware(["auth:sanctum", "abilities:orders:read"]);
```

### Socialite

Turn on the `laravel` crate's `socialite` feature, put the provider's
credentials in `config/services.rs`, and let users sign in with GitHub,
Google, Facebook, X, LinkedIn, GitLab, Bitbucket, or Slack:

```rust
use laravel::socialite::Socialite;

Route::get("/auth/redirect", || async { Socialite::driver("github").redirect() });

Route::get("/auth/callback", || async {
    let github_user = Socialite::driver("github").user().await?;

    let user = User::update_or_create(
        json!({"github_id": github_user.id}),
        json!({"name": github_user.name, "github_token": github_user.token}),
    )
    .await?;

    Auth::login(&user, false).await?;

    Ok::<_, Error>(redirect("/dashboard"))
});
```

### Images

Resize, crop, encode, and store images fluently, in pure Rust:

```rust
let path = request
    .image("avatar")
    .unwrap()
    .orient()
    .cover(400, 400)
    .to_webp()
    .store_publicly_on("avatars", "public")
    .await?;
```

### Queues

Jobs are serializable structs. Dispatch them, chain them, batch them, and
run them with `cargo artisan queue:work`. The `sync`, `database`, `redis`,
`sqs`, `beanstalkd`, `array`, `deferred`, `background`, `failover`, and
`null` drivers are included.

```rust
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProcessPodcast {
    pub id: u64,
}

laravel::register_job!(ProcessPodcast);

#[async_trait]
impl ShouldQueue for ProcessPodcast {
    async fn handle(&self) -> Result<()> {
        // Process the uploaded podcast...
        Ok(())
    }
}

ProcessPodcast { id: 1 }.dispatch().on_queue("podcasts").delay(60).await?;
```

What Laravel declares with interfaces and attributes, jobs declare with
trait methods. A job that returns an id from `unique_id` is unique; one that
returns `Some(DebounceFor::new(30))` from `debounce_for` is debounced, so
when it's dispatched again and again, only the latest dispatch runs.

### Mail and notifications

Mailables are structs too, and Markdown mail is rendered with Laravel's own
components and theme:

```rust
#[derive(Serialize)]
pub struct OrderShipped {
    pub order_id: u64,
}

impl Mailable for OrderShipped {
    fn envelope(&self) -> Envelope {
        Envelope::new().subject("Order Shipped")
    }

    fn content(&self) -> Content {
        Content::markdown("mail.orders.shipped")
    }
}

Mail::to(&user.email).send(OrderShipped { order_id: order.id }).await?;

user.notify(InvoicePaid { invoice_id: invoice.id }).await?;
```

### Testing

Tests make requests against your application, without a server, and read
the way Laravel's feature tests do:

```rust
#[tokio::test]
async fn users_can_log_in() {
    let mut app = crate::app();
    app.refresh_database().await;
    let user = User::factory().create_one().await.unwrap();

    app.post("/login", json!({"email": user.email, "password": "password"}))
        .await
        .assert_redirect("/dashboard");

    app.assert_authenticated_as(&user).await;
}
```

Responses have Laravel's assertions, including fluent JSON assertions:

```rust
app.get_json("/api/users/1")
    .await
    .assert_ok()
    .assert_json_fluent(|json| {
        json.where_("id", 1)
            .where_type("email", "string")
            .missing("password")
            .etc()
    });
```

Uploads are faked the way Laravel fakes them, with real files (fake images
decode):

```rust
let disk = Storage::fake("avatars").unwrap();
let file = UploadedFile::fake().image("avatar.jpg", 200, 200);

app.attach("avatar", file.clone())
    .post("/avatar", json!({"name": "Taylor"}))
    .await
    .assert_ok();

disk.assert_exists(format!("avatars/{}", file.hash_name()).as_str()).await;
```

Views and components can be rendered and tested on their own
(`app.view("welcome", data).assert_see("Laravel")`), and fakes are
available for mail, notifications, the queue, events, HTTP requests,
processes, and more: `Queue::fake()`, `Bus::assert_dispatched::<T>()`,
`Http::fake()`, `Process::fake()`, `Event::fake()`.

## What's included

| Laravel | Crate |
| --- | --- |
| Routing, controllers, middleware, URL generation | `illuminate-routing`, `illuminate-http` |
| Blade templates and components | `illuminate-view` |
| Validation | `illuminate-validation` |
| Query builder, schema builder, migrations, seeders | `illuminate-database` |
| Eloquent, relationships, factories | `illuminate-database` (`eloquent`), `illuminate-macros` |
| Pagination | `illuminate-pagination` |
| Eloquent API resources | `illuminate-http-resources` |
| Authentication, gates, and policies | `illuminate-auth` |
| Sanctum: API tokens and SPA authentication | `laravel-sanctum` (the `sanctum` feature) |
| Socialite: OAuth authentication with GitHub, Google, Facebook, and more | `laravel-socialite` (the `socialite` feature) |
| Sessions, cookies, encryption, hashing | `illuminate-session`, `-cookie`, `-encryption`, `-hashing` |
| Cache (array, file, database, Redis, Memcached, DynamoDB, storage, session, failover) and rate limiting | `illuminate-cache` |
| Redis (with cache, queue, and session drivers) | `illuminate-redis` |
| Queues (database, Redis, SQS, Beanstalkd), jobs, chains, and batches | `illuminate-queue` |
| Broadcasting (Pusher, Reverb, Ably) | `illuminate-broadcasting` |
| Events, logging and `Context`, filesystem, localization | `illuminate-events`, `-log`, `-filesystem`, `-translation` |
| Artisan and task scheduling | `illuminate-console` |
| Processes and concurrency | `illuminate-process`, `illuminate-concurrency` |
| The HTTP client | `illuminate-http-client` |
| Mail (SMTP, sendmail, Postmark, Resend, Mailgun, SES) and Markdown mail | `illuminate-mail` |
| Notifications (mail, database, Slack, custom channels) | `illuminate-notifications` |
| Image manipulation | `illuminate-image` |
| JSON Schema builders | `illuminate-json-schema` |
| Collections, strings, dates, and helpers | `illuminate-support` |
| The application, kernels, exception handling, testing | `illuminate-foundation` |

Applications depend only on the `laravel` crate, which re-exports every
component and provides the prelude.

Still to come:

- **Drivers**: Redis Cluster and TLS connections to Redis aren't supported
  yet, and AWS credentials come only from configuration (there's no
  instance-profile or `~/.aws` credential chain).
- **Packages**: the rest of Laravel's first-party packages (Horizon, Reverb's
  WebSocket server, Scout, and others) and the starter kits.

## Contributing

Every component follows the conventions in
[ARCHITECTURE.md](../ARCHITECTURE.md), and the
[contribution guide](../.github/CONTRIBUTING.md) describes how to work on
the framework.
