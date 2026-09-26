# Loco Inertia DBZ sample

This Loco app is an integration example for `loco-inertia`. It uses Loco's SeaORM integration with a SQLite database and a migration for the DBZ characters. The React pages under `frontend/pages/Characters/` exercise nested data and lists. The `/episodes` page shows an Inertia infinite-scroll episode table. The app's Tera root document is `assets/views/inertia/root.html`; it contains the production CSS and JavaScript tags and uses adapter-provided tags when Vite is running.

## Run the app

Run `cargo run -- db seed` once, then `cargo run -- start`, from this folder. The included development config binds port 5150 and uses the local SQLite file `dbz.sqlite3`. The app registers `InertiaLayer` in `Hooks::after_routes` so the Loco router manages the application routes and middleware.

## Frontend development

For frontend development with HMR, run `npm run dev` in `frontend/`. A small Vite plugin writes Vite's actual resolved URL to `frontend/hot` and removes the file when Vite shuts down. The `loco-inertia` library reads that URL, checks Vite's `/@vite/client` endpoint, and generates the HMR client, React Fast Refresh preamble, and `app.jsx` entry tags. If port 5173 is busy, Vite can choose another port and the hot file records that port. Loco checks the hot file on each full-page response, so Vite can start or stop while Loco is running. Set `INERTIA_VITE_DEV_SERVER` to override the hot-file URL when the browser needs a different address, such as when the services run in separate containers. Vue apps use the same hot-file config without calling `.react_refresh()`.

## Build and run Rust SSR

This sample uses the [inertia-rs-ssr V8 renderer](https://github.com/WalrusSoup/inertia-rs-ssr), which executes the Vite SSR bundle in Rust without a separate Node SSR process. Keep the renderer checkout next to `loco-inertia` in the same `projects` directory. From the `loco-inertia` repository root, run:

```powershell
git clone https://github.com/WalrusSoup/inertia-rs-ssr ../inertia-rs-ssr
```

Build the client assets and SSR bundle:

```powershell
cd frontend
npm install
npm run build
```

Start the renderer in a terminal from `frontend/`:

```powershell
npm run start:ssr
```

Then start Loco from another terminal in this app directory:

```powershell
cargo run -- start
```

The SSR service listens at `http://127.0.0.1:13714/render`; the example's typed settings use that URL by default. The React entry hydrates when SSR markup is present and mounts with `createRoot` when the adapter falls back to client rendering. If the renderer is unavailable, the adapter logs the error and serves the page for client rendering instead.

The `start:ssr` script passes `--debug` to the Rust renderer. When a browser visit uses SSR, its terminal prints a line such as `POST /render component="Characters/Index" -> 200 OK`. To confirm the returned HTML, load `http://localhost:5150/` and inspect the initial document response: the character list and `<title>` should already be present before JavaScript runs. Inertia JSON visits, such as `curl -H 'X-Inertia: true' http://localhost:5150/`, skip SSR.
