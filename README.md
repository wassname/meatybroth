# Meaty Broth

A personal Nostr reader for trying search and feed algorithms on the last
month of text. All content is Nostr — followed RSS/Mastodon bridges included;
there is no native external-site collection. See [SPEC.md](SPEC.md)
for scope and [docs/PROJECT.md](docs/PROJECT.md) for background. Curated notes cover
[Nostr ingestion](docs/research/nostr-ingestion.md), [content policy](docs/research/nostr-content-policy.md),
and [deployment cost and lifecycle](docs/deployment.md).

This is a fresh deployment, separate from The Rusty Claw. All inherited Rusty
Claw deployment targets have been removed.

## Quick start

```bash
just setup     # uv sync from uv.lock
just collect   # one bounded collection pass into .local/meatybroth.db
just serve     # reader at http://127.0.0.1:8081/
just test      # pytest
just smoke     # config tests plus a served-page check
just validate  # every recipe parses (just --dry-run)
```

## Docker

```bash
docker compose up -d --build   # local web on 127.0.0.1:8081 + collector
docker compose logs -f
```

The deployment adds the `public` profile: Caddy serves HTTPS while the app keeps
its loopback diagnostic port.

## First deployment

The domain must have a public Route53 hosted zone in the target AWS account.
Review and publish the current commit, authenticate, then run:

```bash
git push origin main
gh repo edit wassname/meatybroth --visibility public --accept-visibility-change-consequences
aws login --profile your-profile --region us-east-1
AWS_PROFILE=your-profile AWS_ACCOUNT=123456789012 just deploy
```

`just deploy` creates a separate `meatybroth-exp-wassname` CloudFormation stack,
sets the Route53 A record for `meatybroth.com`, starts the app, collector and
Caddy, then checks the corpus and public HTTPS page. It refuses dirty deployment
inputs, an unpublished commit, the wrong AWS account, or a missing hosted zone.
Use `just deploy-prepare` to stop after change-set review. Override defaults with
`AWS_PROFILE`, `AWS_ACCOUNT`, `AWS_REGION`, `STACK_NAME`, or `DOMAIN`.

This is deliberately first-deploy-only while the corpus is disposable. To replace
it, delete the stack (which deletes its corpus) and run `just deploy` again. Add a
non-destructive update command when keeping the collected corpus matters.
