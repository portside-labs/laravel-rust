<p align="center"><a href="https://laravel.com" target="_blank"><img src="https://raw.githubusercontent.com/laravel/art/master/logo-lockup/5%20SVG/2%20CMYK/1%20Full%20Color/laravel-logolockup-cmyk-red.svg" width="400" alt="Laravel Logo"></a></p>

<p align="center">
<a href="https://laravel.com/docs/13.x"><img src="https://img.shields.io/badge/laravel-13.x-FF2D20" alt="Laravel Version"></a>
<a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.89%2B-B7410E" alt="Rust Version"></a>
<a href="illuminate"><img src="https://img.shields.io/badge/components-35-blue" alt="Components"></a>
<a href="LICENSE.md"><img src="https://img.shields.io/badge/license-MIT-brightgreen" alt="License"></a>
</p>

## About Laravel

> **Note:** This repository contains the core code of the Laravel framework, ported to Rust. It's an unofficial port, not affiliated with Laravel. If you want to build an application using Laravel Rust, install the `laravel-rust` installer and create one from the [application skeleton](skeleton):
>
> ```shell
> curl -fsSL https://portsidelabs.io/laravel-rust/install.sh | sh
> laravel-rust new example-app
> ```

Laravel is a web application framework with expressive, elegant syntax. We believe development must be an enjoyable, creative experience to be truly fulfilling. Laravel attempts to take the pain out of development by easing common tasks used in the majority of web projects, such as:

- [Simple, fast routing engine](https://laravel.com/docs/routing).
- [Powerful dependency injection container](https://laravel.com/docs/container).
- Multiple back-ends for [session](https://laravel.com/docs/session) and [cache](https://laravel.com/docs/cache) storage.
- Database agnostic [schema migrations](https://laravel.com/docs/migrations).
- [Robust background job processing](https://laravel.com/docs/queues).
- [Real-time event broadcasting](https://laravel.com/docs/broadcasting).

Laravel is accessible, yet powerful, providing tools needed for large, robust applications. A superb combination of simplicity, elegance, and innovation gives you a complete toolset required to build any application with which you are tasked.

This port brings that toolset to Rust: Eloquent, Blade, validation, queues, Artisan, and testing that reads like a story, with your application compiled to a single fast binary.

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

## Learning Laravel

Laravel has the most extensive and thorough [documentation](https://laravel.com/docs) and video tutorial library of all modern web application frameworks, making it a breeze to get started with the framework. Nearly all of it applies here: method names are the same in snake case (`whereIn` is `where_in`, `firstOrFail` is `first_or_fail`), facades are still facades, and helpers are still helpers. The [guide to Laravel Rust](docs/README.md) tours the differences, and [ARCHITECTURE.md](ARCHITECTURE.md) explains how the framework is put together.

If you're not in the mood to read, [Laracasts](https://laracasts.com) contains thousands of video tutorials covering a range of topics including Laravel, modern PHP, unit testing, JavaScript, and more. Boost the skill level of yourself and your entire team by digging into our comprehensive video library.

You can also watch bite-sized lessons with real-world projects on [Laravel Learn](https://laravel.com/learn), where you will be guided through building a Laravel application from scratch.

## Contributing

Thank you for considering contributing to Laravel Rust! The contribution guide can be found in [CONTRIBUTING.md](.github/CONTRIBUTING.md).

## Code of Conduct

In order to ensure that the Laravel community is welcoming to all, please review and abide by the [Code of Conduct](https://laravel.com/docs/contributions#code-of-conduct).

## Security Vulnerabilities

Please review [our security policy](https://github.com/portside-labs/laravel-rust/security/policy) on how to report security vulnerabilities.

## License

The Laravel framework is open-sourced software licensed under the [MIT license](LICENSE.md).
