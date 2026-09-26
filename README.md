# loco-inertia

An [Inertia.js](https://inertiajs.com/) server adapter for [Loco.rs](https://loco.rs/) and Axum. It sends typed page responses as Inertia JSON or renders the initial HTML document in request middleware.

The default `loco` feature adapts Loco's built-in database pagination into Inertia v3 infinite-scroll props. Disable default features for the Axum-only adapter. Use `.lazy` for expensive props that should be computed on full visits and only when selected during partial reloads.

```rust
use loco_inertia::Inertia;
use serde::Serialize;

#[derive(Serialize)]
struct WelcomeProps { name: String }

async fn welcome() -> Result<impl axum::response::IntoResponse, loco_rs::Error> {
    Inertia::render("Welcome").props(WelcomeProps { name: "Goku".into() })
        .map_err(|e| loco_rs::Error::string(&e.to_string()))
}
```

Install `InertiaLayer::new(config)` on the final Axum router in `Hooks::after_routes`. This lets the layer read request headers and handle controller responses. Map `PropsError`, which implements `std::error::Error`, to the application's error type.

```rust,ignore
async fn after_routes(router: axum::Router, _ctx: &AppContext) -> loco_rs::Result<axum::Router> {
    Ok(InertiaLayer::new(InertiaConfig::default()).layer(router))
}
```

To render the document with Loco, register a renderer that calls the app's `ViewRenderer`:

```rust,ignore
let tera = TeraView::build()?;
let mut config = InertiaConfig::default();
config.asset_tags = "<script type=\"module\" src=\"/assets/app.js\"></script>".into();
let config = config.root_view(move |data| {
    tera.render("inertia/root.html", data.clone()).map_err(|err| err.to_string())
});
```

In the Tera root view, put `{{ inertia_head | safe }}` in `<head>` and `{{ inertia_root | safe }}` in `<body>`. `inertia_root` contains the JSON page script and mount element, plus any SSR body markup. The adapter escapes JSON for a script context, including every forward slash as required by Inertia v3. Without a registered renderer, it uses a minimal document shell. Set `version` from the frontend build manifest. The adapter merges `shared_props` first, then lets page props take precedence. Same-component partial requests select the requested props. A stale-version Inertia GET returns 409 with `X-Inertia-Location`. Mutating Inertia redirects convert 302 to 303.

SSR is off by default. Enable it in the Loco `Hooks::after_routes` setup by setting `InertiaConfig::ssr`. `SsrConfig::url` is the full URL of the Node SSR service's `/render` endpoint, including its host and port. It defaults to `http://127.0.0.1:13714/render`:

```rust,ignore
use loco_inertia::{InertiaConfig, InertiaLayer, SsrConfig};

async fn after_routes(router: axum::Router, _ctx: &AppContext) -> loco_rs::Result<axum::Router> {
    let mut config = InertiaConfig::default();
    config.ssr = Some(SsrConfig {
        url: "http://127.0.0.1:13714/render".into(),
        ..SsrConfig::default()
    });

    Ok(InertiaLayer::new(config).layer(router))
}
```

Run the Inertia-compatible Node SSR process separately from Loco and set `SsrConfig::url` to an address reachable from the Loco process. For example, use `http://127.0.0.1:13714/render` when both processes share a host, or `http://frontend-ssr:13714/render` when the SSR process is a `frontend-ssr` service on the same container network. In Loco apps, put environment-specific values in the typed app config and use environment interpolation there. The adapter POSTs the Inertia page JSON to this endpoint for full browser visits; the service must return JSON with `head` (an array of HTML strings) and `body` (an HTML string). Inertia JSON visits skip SSR. Renderer errors are logged and fall back to client rendering unless `strict` is enabled, in which case the adapter returns 502. The endpoint is trusted to return HTML fragments.

For Vite development, set `InertiaConfig::vite_dev_server` to a `ViteDevConfig`. The library reads the Vite hot file, checks the recorded server URL, and generates the Vite client and entry tags. React projects can opt into the React Fast Refresh preamble with `.react_refresh()`; other framework plugins can use the generic Vite client and their own entry without React-specific code. If Vite is unavailable it uses `asset_tags`, so configure those with production assets:

```rust
use loco_inertia::ViteDevConfig;
use std::path::Path;

config.vite_dev_server = Some(ViteDevConfig::new(
    Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/hot"),
    "app.jsx",
).react_refresh()); // Needed with @vitejs/plugin-react; omit for Vue.
config.asset_tags = r#"<link rel="stylesheet" href="/assets/app.css"><script type="module" src="/assets/app.js"></script>"#.into();
```

Vite must write its resolved URL to the configured hot file. The [example's Vite config](examples/loco-rs-test/frontend/vite.config.js) includes a plugin that records the URL and removes the file when Vite stops. If Vite's preferred port is occupied, the plugin records the port Vite selected. The library checks the hot file on each full-page response, so Vite can start or stop while Loco runs. Use `ViteDevConfig::server_url` to set a different URL when the browser cannot reach the recorded one.

The [`examples/loco-rs-test`](examples/loco-rs-test) app includes a React SSR entry point and scripts to build and run the Node process. See its README for the commands.

## Page directives

`InertiaResponse` supports once props, append/prepend and deep merges, keyed merge matching, flash data, and browser-history flags:

```rust,ignore
let response = Inertia::render("Posts/Index")
    .props(posts)?
    .merge("posts.data")
    .match_on("posts.data.id")
    .once("posts")
    .encrypt_history()
    .flash("notice", "Posts loaded")?;
```

Use `.once_as(prop, cache_key)`, `.once_until(prop, unix_milliseconds)`, or `.once_fresh(prop)` to tune once-prop caching. `.deep_merge(path)` enables recursive merging. `match_on` accepts the full dot path to the identity field used to match incoming rows. `.clear_history()` clears browser history, and `.encrypt_history()` asks the Inertia client to encrypt this page in history; client encryption requires a secure browser context. Flash values are emitted on the current page under `page.flash`. Persisting them across a redirect requires the application's session integration.

## Lazy and deferred props

Props can be evaluated asynchronously only when an Inertia partial reload asks for them. Resolvers are repeatable `Fn` closures and must be `Send + Sync + 'static`; capture shared request data with `Arc`, or clone it into each returned future:

```rust,ignore
let response = Inertia::render("Users/Index")
    .props(serde_json::json!({ "title": "Users" }))?
    .optional("users", move || async move {
        load_users().await
    })
    .deferred("stats", "dashboard", move || async move {
        load_stats().await
    });
```

`optional` props are omitted on regular visits and resolved only when requested with `only` or `except`. `deferred` props are listed in the `deferredProps` page metadata so an Inertia v3 client can request them after the initial render; props with the same group load together. Resolvers return `Result<T, E>` where `T: Serialize` and `E: Error + Send + Sync`. Resolution errors produce a 500 response and preserve their source in `PropsError`. `.always("key", value)` marks a value to remain present during partial reloads. Eager props are still evaluated by the handler before the adapter sees the response.

## Infinite scroll and merge props

Enable the optional `loco` feature to pass Loco's `PageResponse` directly. The adapter converts it to Inertia's `data`/`meta` shape and derives `scrollProps` and merge metadata:

```rust,ignore
let db = db.clone();
let page = pagination.page;
let page_size = pagination.page_size;
let response = Inertia::render("Posts/Index")
    .props(serde_json::json!({}))?
    .infinite_scroll("posts", move || {
        let db = db.clone();
        let pagination = query::PaginationQuery { page, page_size };
        async move { query::paginate(&db, Post::find(), None, &pagination).await }
    });
```

This emits `mergeProps` and `scrollProps`; Loco's `page` and `total_pages` determine previous/next page metadata. The query closure runs on the initial visit and when `posts` is requested in a partial reload. Other expensive props can use `.lazy` so unrelated partial reloads skip their work. The Inertia v3 `<InfiniteScroll data="posts">` component requests additional pages and merges them.

## Example app

[`examples/loco-rs-test`](examples/loco-rs-test) is a runnable Loco integration example.

## Scope

The adapter supports standard page responses, lazy optional and deferred props, always and once props, partial `only`/`except` selection, append/prepend/deep merge metadata, keyed merge matching, infinite-scroll metadata and reset intent, flash data on rendered pages, history clear/encrypt flags, asset version mismatch reloads, Vary merging, redirect status adjustment, script-safe HTML bootstrap, and optional official SSR HTTP transport. Session persistence for flash messages, route exclusions, and dynamic request-aware shared-prop providers remain application concerns or future work.
