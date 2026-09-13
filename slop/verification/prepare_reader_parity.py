"""Snapshot matched content and graph; invoke the unchanged Python reader. -- Pi/gpt-6-astra"""
import json
import re
import sqlite3
import sys
import time
from pathlib import Path

REFERENCE = Path('/workspace/meatybroth')
SDK_GRAPH = '--sdk-graph' in sys.argv
OUT = Path('/workspace/meatybroth-rust/.local') / ('reader-fullgraph' if SDK_GRAPH else 'reader-parity')
OUT.mkdir(parents=True, exist_ok=True)
sys.path.insert(0, str(REFERENCE))
from meatybroth.store import Store
from meatybroth.web import create_app

for name, source in [('baseline', REFERENCE / '.local/verification/pre-sdk-parity/baseline.db'),
                     ('canonical', REFERENCE / '.local/sdk-parity/events.sqlite')]:
    destination = OUT / f'{name}.sqlite'
    destination.unlink(missing_ok=True)
    with sqlite3.connect(f'file:{source}?mode=ro', uri=True) as src, sqlite3.connect(destination) as dst:
        src.backup(dst)
before = sqlite3.connect(OUT / 'baseline.sqlite')
after = sqlite3.connect(OUT / 'canonical.sqlite')
after.execute('PRAGMA foreign_keys=ON')
ids = [{r[0] for r in db.execute('SELECT canonical_id FROM posts')} for db in (before, after)]
common = ids[0] & ids[1]
excluded = {f'nostr:{r[0]}' for r in after.execute('SELECT event_id FROM policy_exclusions')}
nsfw = {r[0] for r in after.execute("SELECT m.value FROM moderation_lists l,json_each(l.members_json) m WHERE l.identifier='nsfw'")}
excluded |= {r[0] for r in before.execute('SELECT canonical_id,author_id FROM posts') if r[1] in nsfw}
common -= excluded
for db, original in zip((before, after), ids):
    db.executemany('DELETE FROM posts WHERE canonical_id=?', [(key,) for key in original-common])

# Hold post fields and profile/graph/warnings fixed to isolate the reader change. -- Pi/gpt-6-astra
if SDK_GRAPH:
    before.execute('DELETE FROM follows')
    before.execute('DELETE FROM nostr_state WHERE kind=3')
    for row in after.execute('SELECT lower(hex(id)),lower(hex(pubkey)),created_at,content,json(tags),lower(hex(sig)) FROM events WHERE kind=3'):
        event = dict(zip(['id', 'pubkey', 'created_at', 'content', 'tags', 'sig'], row))
        event.update(kind=3, tags=json.loads(event['tags']))
        before.execute('INSERT INTO nostr_state VALUES(?,?,?,?,?,?)',
                       (event['pubkey'], 3, event['id'], event['created_at'], json.dumps(event), 'SDK snapshot'))
        follows = {t[1] for t in event['tags'] if len(t) >= 2 and t[0] == 'p' and re.fullmatch('[0-9a-f]{64}', t[1])}
        before.executemany('INSERT INTO follows VALUES(?,?)', [(event['pubkey'], key) for key in follows])
fields = [r[1] for r in before.execute('PRAGMA table_info(posts)')]
after.execute('DELETE FROM posts')
after.executemany(f'INSERT INTO posts({",".join(fields)}) VALUES({",".join("?" for _ in fields)})',
                  before.execute('SELECT * FROM posts').fetchall())
after.execute('DELETE FROM events WHERE kind IN (0,3)')
metadata_count = 0
for (event_json,) in before.execute('SELECT event_json FROM nostr_state WHERE kind IN (0,3)'):
    event = json.loads(event_json)
    after.execute('INSERT INTO events VALUES(?,?,?,?,?,?,?)',
                  (bytes.fromhex(event['id']), bytes.fromhex(event['pubkey']), event['created_at'], event['kind'],
                   event['content'], json.dumps(event['tags']), bytes.fromhex(event['sig'])))
    after.executemany('INSERT OR IGNORE INTO event_tags VALUES(?,?,?)',
                      [(bytes.fromhex(event['id']), t[0], t[1]) for t in event['tags']
                       if len(t) >= 2 and len(t[0]) == 1 and t[0].isascii() and t[0].isalpha()])
    metadata_count += 1
after.execute('DELETE FROM content_warnings')
after.executemany('INSERT INTO content_warnings VALUES(?,?,?,?)', before.execute('SELECT * FROM content_warnings').fetchall())
# Deliberately remove the obsolete metadata/graph tables from the Rust fixture. -- Pi/gpt-6-astra
after.execute('DROP TABLE IF EXISTS nostr_state')
after.execute('DROP TABLE IF EXISTS follows')
for db in (before, after):
    db.commit()
    db.close()

root = '60c052cf19fbfb973c1779585df423e3982a3a251fc826d4c76f8063621c5bb6'
now = int(time.time())
import meatybroth.web
meatybroth.web.timestamp = lambda: now
app = create_app(Store(OUT / 'baseline.sqlite'), root)
app.testing = True
client = app.test_client()
queries = ['mode=new', 'mode=new&page=1', 'mode=new&page=10', '', 'mode=conversations&page=1']
for query in ['bitcoin', '%22local+AI%22', 'security', 'nostr', 'NOT']:
    queries.extend([f'q={query}&mode=relevance', f'q={query}&mode=new', f'q={query}&mode=conversations'])
for reach in [1, 2, 3]:
    for order in ['recent', 'connections']:
        queries.extend([f'mode=discovery&reach={reach}&order={order}',
                        f'mode=discovery&reach={reach}&order={order}&q=bitcoin',
                        f'mode=discovery&reach={reach}&order={order}&page=1'])
queries.extend([f'mode={mode}&before={now-86400}' for mode in ['new', 'conversations', 'discovery']])
cases = []
for query in queries:
    response = client.get('/?' + query)
    assert response.status_code == 200, query
    html = response.get_data(as_text=True)
    cases.append({'path': '/', 'query': query, 'ids': re.findall(r'<article class="post" id="([^"]+)"', html)})
with sqlite3.connect(OUT / 'baseline.sqlite') as db:
    contexts = db.execute('SELECT parent_id FROM posts WHERE parent_id IS NOT NULL GROUP BY parent_id ORDER BY COUNT(*) DESC LIMIT 3').fetchall()
    contexts += db.execute('SELECT canonical_id FROM posts WHERE parent_id IS NOT NULL LIMIT 2').fetchall()
for (cid,) in contexts:
    path = '/context/' + cid.replace(':', '/', 1)
    response = client.get(path)
    if response.status_code == 200:
        cases.append({'path': path, 'query': '', 'ids': re.findall(r'<article class="post" id="([^"]+)"', response.get_data(as_text=True))})
result = {'now': now, 'root': root, 'common_posts': len(common), 'canonical_metadata_events': metadata_count,
          'scope': f'Identical retained post fields and signed metadata; graph source={"current SDK" if SDK_GRAPH else "original baseline"}. Excludes policy/NSFW IDs on both sides. Rust has no nostr_state/follows. Reader comparison, not collector parity.', 'cases': cases}
(OUT / 'expected.json').write_text(json.dumps(result, indent=2))
print(json.dumps({k: v for k, v in result.items() if k != 'cases'}, indent=2))
print(f'{len(cases)} original handler cases; {sum(bool(c["ids"]) for c in cases)} nonempty; expected.json saved')
