"""Run the reader or a bounded collection pass."""

import argparse
import json
import os
from pathlib import Path
import sys
import threading
import time

from loguru import logger

from meatybroth.store import PUBLIC_KEY, Store

ROOT_PUBKEY = "60c052cf19fbfb973c1779585df423e3982a3a251fc826d4c76f8063621c5bb6"
DEFAULT_DB = ".local/meatybroth.db"


def application():
    from meatybroth.web import create_app

    store = Store(os.environ.get("MEATYBROTH_DB", DEFAULT_DB))
    root = os.environ.get("MEATYBROTH_ROOT", ROOT_PUBKEY)
    store.expire()

    def cleanup():
        while True:
            time.sleep(60)
            deleted = store.expire()
            if deleted:
                logger.info("Expired {} post bodies", deleted)

    threading.Thread(target=cleanup, name="monthly-expiry", daemon=True).start()
    return create_app(store, root)


def main():
    parser = argparse.ArgumentParser(description="Meaty Broth personal text reader")
    commands = parser.add_subparsers(dest="command", required=True)
    serve = commands.add_parser("serve", help="Serve the reader with Gunicorn")
    collect = commands.add_parser("collect", help="Collect a bounded Nostr sample (bridges included)")
    for command in (serve, collect):
        command.add_argument("--db", default=os.environ.get("MEATYBROTH_DB", DEFAULT_DB))
        command.add_argument("--root", default=os.environ.get("MEATYBROTH_ROOT", ROOT_PUBKEY))
    serve.add_argument("--host", default="127.0.0.1")
    serve.add_argument("--port", type=int, default=8081)
    collect.add_argument("--max-requests", type=int, default=40)
    collect.add_argument("--loop", action="store_true")
    collect.add_argument("--interval", type=int, default=900)
    args = parser.parse_args()
    if not PUBLIC_KEY.fullmatch(args.root):
        parser.error("--root must be a 64-character lowercase hex public key")
    if args.command == "serve":
        env = {**os.environ, "MEATYBROTH_DB": str(Path(args.db).resolve()), "MEATYBROTH_ROOT": args.root}
        os.execvpe(sys.executable, [sys.executable, "-m", "gunicorn", "--bind", f"{args.host}:{args.port}",
                                  "--workers", "1", "--threads", "4", "--access-logfile", "-",
                                  "meatybroth.__main__:application()"], env)
    if args.interval < 60 or args.max_requests < 1:
        parser.error("--interval must be at least 60 seconds and --max-requests positive")
    from meatybroth.ingest import collect_once

    store = Store(args.db)
    while True:
        store.expire()
        result = collect_once(store, args.root, max_requests=args.max_requests)
        logger.info("Collection: {}", json.dumps(result, ensure_ascii=False))
        if not args.loop:
            break
        time.sleep(args.interval)


if __name__ == "__main__":
    main()
