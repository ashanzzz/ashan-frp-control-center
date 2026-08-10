# Security

The current WebUI is intended for a trusted LAN or VPN. It does not provide built-in multi-user authentication and should not be exposed directly to the public Internet.

Use a scoped Cloudflare API token limited to Zone Read and DNS Edit for the managed zone. Treat the ChmlFrp token and Cloudflare token as secrets; provide them through container environment configuration and never commit `.env`.

The container runs without extra Linux capabilities in the supplied Compose configuration and does not require the Docker socket or privileged mode.

Report security issues privately to the repository owner rather than publishing credentials or exploit details in a public issue.
