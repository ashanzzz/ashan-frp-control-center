# Security

The current WebUI is intended for a trusted LAN or VPN. It does not provide built-in user authentication and should not be exposed directly to the public Internet.

Provider credentials can be configured in WebUI → Settings. Saved ChmlFrp and Cloudflare tokens are persisted in the SQLite database under `/data`; the settings API returns only `*_token_configured` booleans and never returns the stored token value. Password inputs are therefore blank after save. Protect `/data`, backups and the Unraid appdata directory as secrets. At-rest application-level encryption is not currently provided.

Environment variables remain optional first-start bootstrap seeds for backward compatibility. Do not commit `.env` files containing credentials. Once a provider token is stored in SQLite, a restart does not replace it from environment variables.

Use a scoped Cloudflare API token limited to the permissions needed by this application, normally Zone Read and DNS Edit for the managed zone. The WebUI connection test is read-only and does not mutate DNS solely to test write permission.

Provider settings cannot be changed while a global reconcile or failover operation is running. This prevents one global operation from using two credential/configuration snapshots.

The container runs without extra Linux capabilities in the supplied Compose/Unraid configuration and does not require the Docker socket or privileged mode.

Report security issues privately to the repository owner rather than publishing credentials or exploit details in a public issue.
