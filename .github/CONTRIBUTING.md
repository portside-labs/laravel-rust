# Laravel Rust Contribution Guide

Thank you for considering contributing! Laravel's own [contribution guide](https://laravel.com/docs/contributions) applies here too: what follows is what's different about working on the port.

## The Goal

Every component is a port of its Laravel counterpart. A feature behaves the way it does in Laravel: the same names (in snake case), the same configuration, the same defaults, and the same error messages. Where Rust asks for something different, the difference should be the smallest one that reads naturally, and documented on the item that makes it.

Before you start, read [ARCHITECTURE.md](../ARCHITECTURE.md). It describes how Laravel's API is spelled in Rust, how facades and the container work, and which crates each component may depend on.

## Bug Reports

Bug reports should include a title, a clear description of the issue, and as much relevant information as possible, along with a code sample that demonstrates it. When the port behaves differently from Laravel, say what Laravel does.

## Pull Requests

Pull requests without a descriptive title, thorough description, or tests will be closed. Before sending one, run the checks for the crates you changed:

```shell
cargo fmt -p illuminate-routing
cargo clippy -p illuminate-routing --all-targets
cargo test -p illuminate-routing
```

Changes that touch several components, or the `laravel` crate, should pass the whole workspace:

```shell
cargo test --workspace
```

## Coding Style

Code is formatted with `rustfmt` and checked with `clippy`, using the repository's configuration. Public items have doc comments, written the way Laravel's documentation would explain them, with an example where one helps.

## Security Vulnerabilities

If you discover a security vulnerability, please follow the [security policy](SECURITY.md). Don't open a public issue.
