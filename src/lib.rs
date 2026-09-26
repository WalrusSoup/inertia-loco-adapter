//! Inertia.js page responses and protocol middleware for Loco.rs and Axum.
//!
//! Controllers return [`Inertia::render`] pages directly. Register
//! [`InertiaLayer`] on the Loco router so middleware can finalize each page
//! using request headers, the app's root view, and optional SSR.
//!
//! # Example
//!
//! ```rust
//! use loco_inertia::Inertia;
//! use serde::Serialize;
//!
//! #[derive(Serialize)]
//! struct UserProps { name: String }
//!
//! let response = Inertia::render("Users/Show")
//!     .props(UserProps { name: "Goku".into() })?;
//! # Ok::<(), loco_inertia::PropsError>(())
//! ```

#![warn(missing_docs)]

mod config;
mod html;
#[cfg(feature = "loco")]
mod loco;
mod middleware;
mod page;
mod response;
mod ssr;
mod vite;

pub use config::{InertiaConfig, RootViewData};
pub use middleware::InertiaLayer;
pub use page::Page;
pub use response::{Inertia, InertiaResponse, PageBuilder, PropsError};
pub use ssr::{SsrConfig, SsrError, SsrResult};
pub use vite::ViteDevConfig;
