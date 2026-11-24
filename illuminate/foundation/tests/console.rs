use illuminate_foundation::Application;
use illuminate_foundation::testing::TestApp;
use illuminate_routing::Route;

fn test_app() -> (TestApp, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let builder = Application::configure_detached(dir.path()).with_routing(|routing| {
        routing
            .web(|| {
                Route::get("/", || async { "Home" }).name("home");
                Route::get("/users/{user}", || async { "User" }).name("users.show");
            })
            .commands(|| {
                illuminate_console::Artisan::command("greet {name}", |cmd| async move {
                    cmd.info(format!("Hello {}!", cmd.argument("name").unwrap_or_default()));
                    Ok(())
                })
                .purpose("Greet someone");
            });
    });
    (TestApp::new(builder), dir)
}

#[tokio::test]
async fn framework_commands_are_available() {
    let (app, _dir) = test_app();

    app.artisan("inspire").assert_successful().await;
    app.artisan("env")
        .expects_output_to_contain("The application environment is [testing].")
        .assert_successful()
        .await;
    app.artisan("route:list")
        .expects_output_to_contain("users/{user}")
        .expects_output_to_contain("users.show")
        .expects_output_to_contain("storage.local")
        .expects_output_to_contain("storage.local.upload")
        .expects_output_to_contain("Showing [4] routes")
        .assert_successful()
        .await;
    app.artisan("about")
        .expects_output_to_contain("Laravel Version")
        .assert_successful()
        .await;
    app.artisan("key:generate --show")
        .expects_output_to_contain("base64:")
        .assert_successful()
        .await;
    app.artisan("greet Taylor")
        .expects_output("Hello Taylor!")
        .assert_successful()
        .await;
}

#[tokio::test]
async fn maintenance_mode_can_be_toggled() {
    let (mut app, _dir) = test_app();

    app.artisan("down --secret=letmein")
        .expects_output_to_contain("Application is now in maintenance mode.")
        .assert_successful()
        .await;
    app.get("/").await.assert_service_unavailable();
    app.get("/letmein").await.assert_redirect("/");
    app.get("/").await.assert_ok().assert_see("Home");

    app.artisan("up")
        .expects_output_to_contain("Application is now live.")
        .assert_successful()
        .await;
    app.flush_cookies();
    app.get("/").await.assert_ok();
}

#[tokio::test]
async fn generators_create_and_register_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("app/http")).unwrap();
    std::fs::write(dir.path().join("app/mod.rs"), "pub mod http;\n").unwrap();
    std::fs::write(dir.path().join("app/http/mod.rs"), "").unwrap();
    std::fs::create_dir_all(dir.path().join("bootstrap")).unwrap();
    std::fs::write(
        dir.path().join("bootstrap/providers.rs"),
        "pub fn providers() -> Vec<Box<dyn ServiceProvider>> {\n    vec![\n        Box::new(crate::app::providers::AppServiceProvider),\n    ]\n}\n",
    )
    .unwrap();
    let app = TestApp::new(Application::configure_detached(dir.path()));

    app.artisan("make:controller Admin/PhotoController --resource")
        .expects_output_to_contain("Controller [app/http/controllers/admin/photo_controller.rs] created successfully.")
        .assert_successful()
        .await;
    let controller = std::fs::read_to_string(dir.path().join("app/http/controllers/admin/photo_controller.rs")).unwrap();
    assert!(controller.contains("impl ResourceController for PhotoController"));
    assert!(std::fs::read_to_string(dir.path().join("app/http/mod.rs")).unwrap().contains("pub mod controllers;"));
    assert!(std::fs::read_to_string(dir.path().join("app/http/controllers/admin/mod.rs")).unwrap().contains("pub use photo_controller::PhotoController;"));

    app.artisan("make:controller Admin/PhotoController")
        .expects_output_to_contain("Controller already exists.")
        .assert_failed()
        .await;

    app.artisan("make:command SendEmails").assert_successful().await;
    let command = std::fs::read_to_string(dir.path().join("app/console/commands/send_emails.rs")).unwrap();
    assert!(command.contains("\"app:send-emails\""));

    app.artisan("make:provider RiakServiceProvider").assert_successful().await;
    let providers = std::fs::read_to_string(dir.path().join("bootstrap/providers.rs")).unwrap();
    assert!(providers.contains("Box::new(crate::app::providers::RiakServiceProvider),"));

    app.artisan("make:view users.index").assert_successful().await;
    assert!(dir.path().join("resources/views/users/index.blade.html").exists());
}

#[tokio::test]
async fn models_can_be_generated_with_their_companions() {
    let (app, dir) = test_app();

    app.artisan("make:model Flight --all")
        .expects_output_to_contain("Model [app/models/flight.rs] created successfully.")
        .expects_output_to_contain("Factory [database/factories/flight_factory.rs] created successfully.")
        .expects_output_to_contain("_create_flights_table.rs] created successfully.")
        .expects_output_to_contain("Seeder [database/seeders/flight_seeder.rs] created successfully.")
        .expects_output_to_contain("Controller [app/http/controllers/flight_controller.rs] created successfully.")
        .expects_output_to_contain("Policy [app/policies/flight_policy.rs] created successfully.")
        .assert_successful()
        .await;

    let read = |path: &str| std::fs::read_to_string(dir.path().join(path)).unwrap();
    assert!(read("app/models/flight.rs").contains("#[use_factory(FlightFactory)]"));
    assert!(read("app/models/mod.rs").contains("pub use flight::Flight;"));
    assert!(read("database/factories/flight_factory.rs").contains("type Model = Flight;"));
    assert!(read("app/policies/flight_policy.rs").contains("impl Policy<Flight> for FlightPolicy"));
    assert!(read("app/http/controllers/flight_controller.rs").contains("async fn destroy"));

    app.artisan("make:observer FlightObserver")
        .expects_output_to_contain("Observer [app/observers/flight_observer.rs] created successfully.")
        .assert_successful()
        .await;
    assert!(read("app/observers/flight_observer.rs").contains("impl Observer<Flight> for FlightObserver"));

    app.artisan("make:model Flight")
        .expects_output_to_contain("Model already exists.")
        .assert_failed()
        .await;
}

#[tokio::test]
async fn events_and_their_listeners_can_be_listed() {
    let (app, _dir) = test_app();
    illuminate_events::Event::listen_named("podcast.processed", |_: &str, _: &illuminate_support::Value| async { Ok(()) });

    app.artisan("event:list")
        .expects_output_to_contain("podcast.processed")
        .assert_successful()
        .await;

    app.artisan("event:list --event=nothing-matches")
        .expects_output_to_contain("Your application doesn't have any events matching the given criteria.")
        .assert_successful()
        .await;
}

#[tokio::test]
async fn cache_items_can_be_forgotten() {
    let (app, _dir) = test_app();
    illuminate_cache::Cache::put("podcast", "Laravel", 60).await.unwrap();

    app.artisan("cache:forget podcast")
        .expects_output_to_contain("The [podcast] key has been removed from the cache.")
        .assert_successful()
        .await;

    assert!(!illuminate_cache::Cache::has("podcast").await.unwrap());
}

#[tokio::test]
async fn environment_files_can_be_encrypted_and_decrypted() {
    use base64::Engine;
    let (app, dir) = test_app();
    let env = dir.path().join(".env");
    let contents = "APP_NAME=Laravel\n# Mail\nMAIL_PASSWORD=secret\n";
    std::fs::write(&env, contents).unwrap();
    let key = format!("base64:{}", base64::engine::general_purpose::STANDARD.encode([7u8; 32]));

    app.artisan(&format!("env:encrypt --key={key}"))
        .expects_output_to_contain("Environment successfully encrypted.")
        .assert_successful()
        .await;
    let encrypted = std::fs::read_to_string(dir.path().join(".env.encrypted")).unwrap();
    assert!(!encrypted.contains("secret"));

    app.artisan(&format!("env:encrypt --key={key}"))
        .expects_output_to_contain("Encrypted environment file already exists.")
        .assert_failed()
        .await;

    std::fs::remove_file(&env).unwrap();
    app.artisan(&format!("env:decrypt --key={key}"))
        .expects_output_to_contain("Environment successfully decrypted.")
        .assert_successful()
        .await;
    assert_eq!(std::fs::read_to_string(&env).unwrap(), contents);

    app.artisan("env:decrypt --key=base64:d3Jvbmcta2V5LXdyb25nLWtleS13cm9uZy1rZXktMTI= --force")
        .assert_failed()
        .await;

    // Readable files encrypt each value, keeping the ciphertext of unchanged ones.
    app.artisan(&format!("env:encrypt --key={key} --readable --force"))
        .assert_successful()
        .await;
    let readable = std::fs::read_to_string(dir.path().join(".env.encrypted")).unwrap();
    assert!(readable.starts_with("APP_NAME="));
    let app_name = readable.lines().next().unwrap().to_string();

    std::fs::write(&env, "APP_NAME=Laravel\nMAIL_PASSWORD=changed\n").unwrap();
    app.artisan(&format!("env:encrypt --key={key} --readable"))
        .assert_successful()
        .await;
    let updated = std::fs::read_to_string(dir.path().join(".env.encrypted")).unwrap();
    assert_eq!(updated.lines().next().unwrap(), app_name);
    assert_ne!(updated.lines().nth(1).unwrap(), readable.lines().nth(1).unwrap());

    std::fs::remove_file(&env).unwrap();
    app.artisan(&format!("env:decrypt --key={key}")).assert_successful().await;
    assert_eq!(std::fs::read_to_string(&env).unwrap(), "APP_NAME=Laravel\nMAIL_PASSWORD=changed\n");
}

#[tokio::test]
async fn mailables_and_notifications_can_be_generated() {
    let (app, dir) = test_app();
    let read = |path: &str| std::fs::read_to_string(dir.path().join(path)).unwrap();

    app.artisan("make:mail OrderShipped --markdown=mail.orders.shipped")
        .expects_output_to_contain("Mailable [app/mail/order_shipped.rs] created successfully.")
        .expects_output_to_contain("View [resources/views/mail/orders/shipped.blade.html] created successfully.")
        .assert_successful()
        .await;
    assert!(read("app/mail/order_shipped.rs").contains(r#"Content::markdown("mail.orders.shipped")"#));
    assert!(read("app/mail/order_shipped.rs").contains(r#"Envelope::new().subject("Order Shipped")"#));
    assert!(read("resources/views/mail/orders/shipped.blade.html").contains("<x-mail::message>"));

    app.artisan("make:notification InvoicePaid")
        .expects_output_to_contain("Notification [app/notifications/invoice_paid.rs] created successfully.")
        .assert_successful()
        .await;
    assert!(read("app/notifications/invoice_paid.rs").contains("impl Notification for InvoicePaid"));

    app.artisan("make:notification InvoicePaid")
        .expects_output_to_contain("Notification already exists.")
        .assert_failed()
        .await;

    app.artisan("make:notifications-table")
        .expects_output_to_contain("Migration created successfully.")
        .assert_successful()
        .await;
}

#[tokio::test]
async fn resources_can_be_generated() {
    let (app, dir) = test_app();
    let read = |path: &str| std::fs::read_to_string(dir.path().join(path)).unwrap();

    app.artisan("make:model Podcast").assert_successful().await;
    app.artisan("make:resource PodcastResource")
        .expects_output_to_contain("Resource [app/http/resources/podcast_resource.rs] created successfully.")
        .assert_successful()
        .await;
    assert!(read("app/http/resources/podcast_resource.rs").contains("type Model = Podcast;"));

    app.artisan("make:resource PodcastCollection")
        .assert_successful()
        .await;
    assert!(read("app/http/resources/podcast_collection.rs").contains("type Collects = PodcastResource;"));

    app.artisan("make:resource Stats --collection").assert_successful().await;
    assert!(read("app/http/resources/stats.rs").contains("impl ResourceCollection for Stats"));
}

#[tokio::test]
async fn the_api_can_be_installed() {
    let (app, dir) = test_app();
    let path = |path: &str| dir.path().join(path);
    let read = |path: &str| std::fs::read_to_string(dir.path().join(path)).unwrap();
    std::fs::write(path("Cargo.toml"), "[package]\nname = \"app\"\n\n[dependencies]\nlaravel = \"0.1\"\n").unwrap();
    std::fs::create_dir_all(path("routes")).unwrap();
    std::fs::write(path("routes/mod.rs"), "pub mod web;\n\npub use web::web;\n").unwrap();
    std::fs::create_dir_all(path("bootstrap")).unwrap();
    std::fs::write(
        path("bootstrap/app.rs"),
        "        .with_routing(|routing| {\n            routing\n                .web(routes::web)\n                .health(\"/up\");\n        })\n",
    )
    .unwrap();

    app.artisan("install:api")
        .expects_output_to_contain("Published API routes file.")
        .expects_output_to_contain("Published the personal access tokens migration.")
        .assert_successful()
        .await;

    assert!(read("Cargo.toml").contains(r#"laravel = { version = "0.1", features = ["sanctum"] }"#));
    assert!(read("routes/api.rs").contains(r#".middleware("auth:sanctum")"#));
    assert!(read("routes/mod.rs").contains("pub mod api;"));
    assert!(read("routes/mod.rs").contains("pub use api::api;"));
    assert!(read("bootstrap/app.rs").contains(".web(routes::web)\n                .api(routes::api)"));
    let migrations = || {
        std::fs::read_dir(path("database/migrations"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(migrations().len(), 1);
    assert!(migrations()[0].ends_with("_create_personal_access_tokens_table.rs"));

    // Installing again leaves everything as it is.
    app.artisan("install:api")
        .doesnt_expect_output_to_contain("Published")
        .assert_successful()
        .await;
    assert_eq!(migrations().len(), 1);
    assert_eq!(read("bootstrap/app.rs").matches("routes::api").count(), 1);
}

#[tokio::test]
async fn the_schedule_is_paused_through_the_cache() {
    let (app, _dir) = test_app();

    app.artisan("schedule:pause").assert_successful().await;
    assert_eq!(
        illuminate_cache::Cache::get("illuminate:schedule:paused").await.unwrap(),
        Some(illuminate_support::json!(true))
    );
    assert!(illuminate_console::Schedule::instance().is_paused().await);

    app.artisan("schedule:resume").assert_successful().await;
    assert!(!illuminate_console::Schedule::instance().is_paused().await);
}

#[tokio::test]
async fn more_classes_can_be_generated() {
    let (app, dir) = test_app();
    let read = |path: &str| std::fs::read_to_string(dir.path().join(path)).unwrap();

    app.artisan("make:scope AncientScope")
        .expects_output_to_contain("Scope [app/models/scopes/ancient_scope.rs] created successfully.")
        .assert_successful()
        .await;
    assert!(read("app/models/scopes/ancient_scope.rs").contains("impl<M: Model> Scope<M> for AncientScope"));

    app.artisan("make:channel OrderChannel").assert_successful().await;
    assert!(read("app/broadcasting/order_channel.rs").contains("pub async fn join(_user: AuthUser) -> bool"));

    app.artisan("make:job-middleware RateLimited").assert_successful().await;
    assert!(read("app/jobs/middleware/rate_limited.rs").contains("impl JobMiddleware for RateLimited"));

    std::fs::create_dir_all(dir.path().join("config")).unwrap();
    std::fs::write(dir.path().join("config/mod.rs"), "laravel::config_files![app, mail];\n").unwrap();
    app.artisan("make:config Billing")
        .expects_output_to_contain("Config [config/billing.rs] created successfully.")
        .assert_successful()
        .await;
    assert!(read("config/billing.rs").contains("pub fn config() -> Value"));
    assert_eq!(read("config/mod.rs"), "laravel::config_files![app, billing, mail];\n");
}

#[tokio::test]
async fn storage_unlink_matches_laravel_for_directory_symlinks() {
    let (app, dir) = test_app();
    let target = dir.path().join("storage/app/public");
    let file = target.join("nested/keep.txt");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "Keep this file").unwrap();

    app.artisan("storage:link").assert_successful().await;
    let link = dir.path().join("public/storage");
    assert!(link.is_symlink());
    assert_eq!(
        std::fs::read_to_string(link.join("nested/keep.txt")).unwrap(),
        "Keep this file"
    );

    app.artisan("storage:unlink")
        .expects_output_to_contain("link has been deleted.")
        .assert_successful()
        .await;

    // Laravel reports success on Windows even if unlink cannot remove a directory symlink.
    assert_eq!(link.is_symlink(), cfg!(windows));
    assert_eq!(link.exists(), cfg!(windows));
    assert!(target.is_dir());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Keep this file");
}

#[tokio::test]
async fn storage_unlink_leaves_real_directories_alone() {
    let (app, dir) = test_app();
    let link = dir.path().join("public/storage");
    let file = link.join("keep.txt");
    std::fs::create_dir_all(&link).unwrap();
    std::fs::write(&file, "Keep this file").unwrap();

    app.artisan("storage:unlink")
        .doesnt_expect_any_output()
        .assert_successful()
        .await;

    assert!(link.is_dir());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Keep this file");
}

#[tokio::test]
async fn storage_unlink_removes_file_symlinks_when_configured() {
    let (app, dir) = test_app();
    let link = dir.path().join("public/storage");
    let file = dir.path().join("keep.txt");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::fs::write(&file, "Keep this file").unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(&file, &link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_file(&file, &link).unwrap();

    app.app()
        .override_config("filesystems.links", illuminate_support::json!({}));
    app.artisan("storage:unlink")
        .doesnt_expect_any_output()
        .assert_successful()
        .await;
    assert!(link.is_symlink());

    app.app().override_config(
        "filesystems.links",
        illuminate_support::json!({ (link.to_string_lossy()): file.to_string_lossy() }),
    );
    app.artisan("storage:unlink")
        .expects_output_to_contain("link has been deleted.")
        .assert_successful()
        .await;
    assert!(!link.is_symlink());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Keep this file");

    app.artisan("storage:unlink")
        .doesnt_expect_any_output()
        .assert_successful()
        .await;
}

#[cfg(windows)]
#[tokio::test]
async fn storage_unlink_skips_windows_junctions() {
    let (app, dir) = test_app();
    let link = dir.path().join("public").join("storage");
    let target = dir.path().join("storage").join("app").join("public");
    let file = target.join("keep.txt");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(&file, "Keep this file").unwrap();

    let result = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .unwrap();
    assert!(result.status.success(), "{:?}", result);
    assert_eq!(
        std::fs::read_to_string(link.join("keep.txt")).unwrap(),
        "Keep this file"
    );

    app.artisan("storage:unlink")
        .doesnt_expect_any_output()
        .assert_successful()
        .await;

    assert_eq!(
        std::fs::read_to_string(link.join("keep.txt")).unwrap(),
        "Keep this file"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Keep this file");
}

#[tokio::test]
async fn packages_are_discovered_and_reset_tokens_cleared() {
    let (app, _dir) = test_app();

    app.artisan("package:discover")
        .expects_output_to_contain("Discovering packages")
        .assert_successful()
        .await;

    app.app().override_config("auth.passwords.users.driver", "cache");
    app.artisan("auth:clear-resets")
        .expects_output_to_contain("Expired reset tokens cleared successfully.")
        .assert_successful()
        .await;
}
