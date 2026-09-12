# Built from the checked-out source, never pulled from a mutable branch.
# syntax=docker/dockerfile:1

# python:3.12-slim-bookworm, digest resolved from Docker Hub 2026-09-12
FROM python@sha256:782412e85d0f0984994c290652577d4018aff08145c85b262bb63dc0c7522254

# uv pinned so the uv.lock resolution is reproducible in the image
COPY --from=ghcr.io/astral-sh/uv:0.11.3 /uv /uvx /usr/local/bin/

ENV UV_LINK_MODE=copy
WORKDIR /opt/meatybroth

# Dependencies first for layer caching; uv.lock is the only source of versions
COPY pyproject.toml uv.lock ./
RUN uv sync --frozen --no-dev

COPY meatybroth/ meatybroth/

ENV PATH="/opt/meatybroth/.venv/bin:$PATH"
EXPOSE 8081
# SQLite database path is /var/lib/meatybroth, initialized by Store() on first use
CMD ["python", "-m", "meatybroth", "serve", "--db", "/var/lib/meatybroth/meatybroth.db", "--host", "0.0.0.0", "--port", "8081"]
