# ADR 0002: Static WebUI served by Axum

**Status:** Accepted

The operations console uses static HTML, CSS and browser ES modules. Dioxus/WASM was removed in 0.2.0 because the application does not need a compiled frontend and the second Rust/WASM/Dioxus build pipeline repeatedly prevented releases. The API remains the product boundary.
