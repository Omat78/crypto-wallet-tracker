# Crypto Wallet Tracker (Rust / Axum)

Wallet & portfolio tracker: give it an address, get holdings, transaction
history, and P&L across Ethereum and Solana. Real-time alerts (price moves,
large transfers) are gated behind a paid tier.

## Stack
- **Axum** (async web framework) + **Tokio**
- **SQLite via sqlx** — users, API keys, alert subscriptions
- **reqwest** — Etherscan API, Solana JSON-RPC, CoinGecko free-tier price API
- Background **tokio task** polling alerts and firing webhooks (Discord/Slack/generic)
- **Static dashboard** (`static/index.html`, vanilla HTML/CSS/JS, no build step) served
  directly by Axum via `tower_http::services::ServeDir` — one process, one deploy.

## Dashboard
Visiting the server's root URL (e.g. `http://localhost:8080`) loads a single-page
dashboard: paste a wallet address, see holdings/transactions/P&L, and (once
connected with a paid key) manage real-time alerts. It talks to the same
`/api/*` routes documented below and stores the API key in the browser's
`localStorage`. No separate frontend host or build pipeline needed — it's
just a static file the Rust server serves.

## Local setup
```bash
cp .env.example .env
# add your free Etherscan API key: https://etherscan.io/apis
cargo run
```
Server starts on `http://localhost:8080`.

## API

### Auth
All wallet/alert endpoints require an `x-api-key` header.

```bash
curl -X POST localhost:8080/api/signup -H "Content-Type: application/json" \
  -d '{"email":"you@example.com"}'
# => { "api_key": "cwt_...", "message": "..." }
```

New users are free-tier by default.

`GET /api/me` returns `{ email, is_paid }` for the authenticated key — the
dashboard uses this to decide whether to show the alerts form or the upgrade
notice.

`POST /api/recover { email }` — issues a brand-new API key for that email
and deactivates the old one, emailing the new key via Resend (or logging it
server-side if `RESEND_API_KEY` isn't set — see "Email recovery" below).
Always returns the same generic message whether or not the email has an
account, so this endpoint can't be used to check which emails are registered.

### Going paid (real Stripe Checkout)
```
POST /api/checkout          (auth required) -> { "checkout_url": "https://checkout.stripe.com/..." }
POST /api/stripe/webhook    (Stripe calls this, not you)
```
`POST /api/checkout` creates a real Stripe Checkout Session for the
authenticated user and returns the URL to redirect them to — the dashboard's
"Upgrade" button does exactly this. After payment, Stripe redirects the
browser back to `{APP_BASE_URL}/?checkout=success` and, separately, calls
`/api/stripe/webhook` — which is what actually flips `is_paid` to `true`
once the signature is verified. See "Stripe setup" below for the one-time
dashboard configuration this needs.

### Wallet data (free tier)
```
GET /api/wallet/{chain}/{address}/holdings      chain = "ethereum" | "solana"
GET /api/wallet/{chain}/{address}/transactions
GET /api/wallet/{chain}/{address}/pnl
```
All require `x-api-key`. Example:
```bash
curl localhost:8080/api/wallet/ethereum/0xabc.../holdings -H "x-api-key: cwt_..."
```

### Real-time alerts (paid tier)
```
POST   /api/alerts        { chain, address, alert_type, threshold, webhook_url }
GET    /api/alerts
DELETE /api/alerts/{id}
```
`alert_type` is `"price_move"` (threshold = % change) or `"large_transfer"`
(threshold = USD value). Non-paid users get `402 Payment Required`. The
background worker polls every `ALERT_POLL_SECONDS` and POSTs a JSON payload
(`{"content": "...", "text": "..."}` — compatible with Discord/Slack incoming
webhooks) to `webhook_url` when a threshold is crossed.

## Stripe setup (one-time, in your Stripe Dashboard)
1. Create a recurring **Price** for the alerts subscription (Products →
   Add product → set a recurring price). Copy its ID (`price_...`) into
   `STRIPE_PRICE_ID`.
2. Copy your **Secret key** (Developers → API keys) into `STRIPE_SECRET_KEY`.
3. Add a **webhook endpoint** pointing at `https://yourapp.com/api/stripe/webhook`
   (Developers → Webhooks → Add endpoint), subscribed to at least
   `checkout.session.completed`. Copy its **Signing secret** (`whsec_...`)
   into `STRIPE_WEBHOOK_SECRET`.
4. Set `APP_BASE_URL` to your real public URL so success/cancel redirects
   land back on your own dashboard instead of `localhost`.

Use Stripe's test mode (test API keys + test card `4242 4242 4242 4242`) to
verify the whole flow before switching to live keys.

**Known timing caveat:** the dashboard checks `is_paid` immediately after
the browser redirects back from Checkout, but Stripe's webhook (which is
what actually flips the flag) can arrive a moment later. If someone sees
"free tier" right after paying, refreshing the page after a few seconds
picks up the webhook's update — an automatic retry/poll would be a nice
follow-up if this causes confusion in practice.

## Email recovery (via Resend)
Losing an API key currently means re-signing up with a new email — `/api/recover`
fixes that by issuing (and emailing) a fresh key for an existing account.
1. Create a free account at https://resend.com and get an API key.
2. Set `RESEND_API_KEY` in your `.env`.
3. Set `EMAIL_FROM` to a sender address verified in your Resend account
   (their sandbox domain `onboarding@resend.dev` works for testing without
   verifying your own domain).

If `RESEND_API_KEY` is left blank, recovery still works end-to-end for
local development — the new key is written to the server's logs instead of
emailed, so you can copy it from there. Don't rely on that fallback once
this is public, since anyone with log access could read it.


- **Stripe webhooks are signature-verified** (`src/security.rs`) against the raw
  request body using HMAC-SHA256, with a 5-minute timestamp tolerance to
  block replay. Requests without a valid `Stripe-Signature` are rejected
  with 400 before any database write happens.
- **API keys are hashed** (SHA-256) before being stored — the raw key is
  shown once at signup and never persisted. Lookups hash the incoming
  `x-api-key` header and match against the stored hash.
- **Rate limiting** is a self-contained fixed-window limiter (60 requests/IP/
  minute by default — see `src/rate_limit.rs`), applied to all `/api/*`
  routes. It reads `X-Forwarded-For` first so it works correctly behind
  Render's proxy; it falls back to the raw socket address for local/direct
  connections.
- **CORS is restricted** via `ALLOWED_ORIGINS` (comma-separated list in
  `.env`). If left unset, it falls back to wide-open CORS and logs a warning
  on startup — set this before pointing a real frontend domain at a public
  deployment.

## Known limitations (documented, not hidden)
- **Token balances (ETH chain)** are approximated by netting ERC-20 transfer
  events from Etherscan, not a live per-token `balanceOf` call. Fine for a
  demo/MVP; swap in Alchemy/Covalent token-balance endpoints for accuracy.
- **P&L** uses current price as a cost-basis proxy (see `src/services/pnl.rs`).
  Accurate realized gains need FIFO/LIFO lot tracking against historical
  price-at-transfer-time — plug in a tx-indexing API (Covalent, Zerion) to
  upgrade this.
- **Solana transaction amounts** are resolved via a `getTransaction` RPC call
  per signature, capped at the most recent `AMOUNT_RESOLUTION_LIMIT` (20, in
  `src/services/solana.rs`) to avoid hammering the free public RPC endpoint.
  Wallets with more than 20 transactions will show "unknown"/0 for older
  ones, and Solana P&L only reflects those 20 — increase the constant (and
  consider a paid RPC provider like Helius/QuickNode) if you need more.
- **Solana SPL token holdings** only recognize a small hardcoded set of mints
  (USDC, USDT, mSOL — see `solana::known_spl_token`). Unrecognized tokens
  are silently omitted rather than shown with no price. Swap in the Jupiter
  token list API for broader coverage.
- **Large-transfer alerts** check only the most recent transaction per poll;
  track last-seen tx hash per alert to avoid duplicate fires in production.
- No persistent database migrations tool — schema changes (like the
  `api_key_hash` column) are applied via `CREATE TABLE IF NOT EXISTS` in
  `src/db.rs`, so an existing `data.db` from an older version of this app
  won't automatically get new columns. Delete `data.db` (or add an `ALTER
  TABLE`) if you're upgrading a running deployment.
- Rate limiting is in-memory per instance — if you scale to multiple server
  instances, move it to something shared (Redis) or a proxy-level limiter.
- **Key recovery has no proof of ownership beyond "knows the email"** — same
  trust model as signup itself (which also never verifies email ownership).
  Good enough for an MVP; add an email confirmation link/OTP step before
  this is handling anything sensitive.

## Deploying to Render
This repo includes a `Dockerfile` and `render.yaml` blueprint:
1. Push to GitHub.
2. In Render: New → Blueprint → point at the repo (it reads `render.yaml`).
3. Set the secret env vars it prompts for (`sync: false` in the blueprint):
   `ETHERSCAN_API_KEY`, `ALLOWED_ORIGINS`, `APP_BASE_URL`, `STRIPE_SECRET_KEY`,
   `STRIPE_PRICE_ID`, `STRIPE_WEBHOOK_SECRET`, `RESEND_API_KEY`, `EMAIL_FROM`.
4. The attached 1GB disk persists `data.db` (SQLite) across deploys.
5. Once deployed, update your Stripe webhook endpoint URL and `APP_BASE_URL`
   to match the real `https://yourapp.onrender.com` address Render gives you.

For higher traffic, swap SQLite for managed Postgres (Render Postgres) —
only `src/db.rs` needs to change (swap `sqlx::sqlite` for `sqlx::postgres`).

## Monetization notes
- Free tier: on-demand holdings/transactions/P&L lookups.
- Paid tier: real-time webhook alerts (price moves + large transfers),
  purchased through an actual Stripe Checkout flow (see "Stripe setup" above)
  — this is wired end-to-end now, not a stub.
