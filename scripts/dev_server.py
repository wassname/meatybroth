"""Dev server with genuine browser live reload (dev-only; not for production).

Watches Python, templates and static files; livereload injects a small script
that refreshes the browser on changes. Run: just dev
"""
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))  # script dir != repo root

from livereload import Server

from meatybroth.store import Store
from meatybroth.web import create_app

ROOT_KEY = os.environ.get("MEATYBROTH_ROOT",
                          "60c052cf19fbfb973c1779585df423e3982a3a251fc826d4c76f8063621c5bb6")
store = Store(os.environ.get("MEATYBROTH_DB", ".local/meatybroth.db"))
app = create_app(store, ROOT_KEY)
app.config["TEMPLATES_AUTO_RELOAD"] = True  # template edits apply without restart

server = Server(app)  # WSGI wrapper with injected livereload script
server.watch("meatybroth/templates")
server.watch("meatybroth/static")
server.watch("meatybroth")  # .py changes restart the app
# tornado.autoreload watches the imported .py files and re-execs the process
# on change (NOT the interactive debugger); the livereload add_reload_hook
# then stops the loop cleanly and the browser refreshes via websocket.
import tornado.autoreload
tornado.autoreload.start()
server.serve(port=int(os.environ.get("DEV_PORT", "8082")),
             host=os.environ.get("DEV_HOST", "127.0.0.1"), debug=True)
