"""Isolated reference server and paired browser checks; no application dependency. -- Pi/gpt-6-astra"""
import asyncio
import json
import sys
from pathlib import Path

ROOT = Path('/workspace/meatybroth-rust')
DATA = ROOT / '.local/reader-parity'
OUT = ROOT / 'slop/verification/reader-browser'

if sys.argv[1] == 'serve-reference':
    sys.path.insert(0, '/workspace/meatybroth')
    from meatybroth.store import Store
    from meatybroth.web import create_app
    from wsgiref.simple_server import make_server
    root = json.loads((DATA / 'expected.json').read_text())['root']
    app = create_app(Store(DATA / 'baseline.sqlite'), root)
    print('Isolated Python baseline ready at http://localhost:8084', flush=True)
    make_server('127.0.0.1', 8084, app).serve_forever()
elif sys.argv[1] == 'measure':
    import re
    import time
    import urllib.request
    rows = []
    for label, port in [('Python matched', 8084), ('Rust matched', 8083), ('Rust original SDK', 8085)]:
        for path in ['/?mode=new', '/?q=bitcoin', '/?mode=discovery&reach=3&order=connections', '/?mode=conversations', '/status']:
            seconds = []
            for _ in range(3):
                start = time.perf_counter()
                with urllib.request.urlopen(f'http://localhost:{port}{path}', timeout=30) as response:
                    html = response.read().decode()
                    assert response.status == 200
                seconds.append(round(time.perf_counter() - start, 4))
                ids = re.findall(r'<article class="post" id="([^"]+)"', html)
                assert path == '/status' or len(ids) == 50
            rows.append({'reader': label, 'path': path, 'seconds': seconds, 'ids': ids})
    result = {'scope': 'Sequential localhost HTTP requests, Rust debug build. Matched snapshots have 6536 posts/54 follow lists; original SDK has 7580 posts/863 follow lists. Not a production capacity estimate.', 'rows': rows}
    (ROOT / 'slop/verification/reader-timing-final.json').write_text(json.dumps(result, indent=2))
    print(json.dumps([{'reader': r['reader'], 'path': r['path'], 'seconds': r['seconds']} for r in rows], indent=2))
else:
    from playwright.async_api import async_playwright

    async def capture():
        OUT.mkdir(exist_ok=True)
        findings = []
        expected = json.loads((DATA / 'expected.json').read_text())
        context_path = next(c['path'] for c in expected['cases'] if c['path'].startswith('/context/'))
        async with async_playwright() as p:
            browser = await p.chromium.connect_over_cdp('http://localhost:9333')
            context = await browser.new_context(viewport={'width': 1280, 'height': 900})
            page = await context.new_page()
            requests = []
            page.on('request', lambda r: requests.append({'url': r.url, 'type': r.resource_type}))
            for label, base in [('before', 'http://localhost:8084'), ('after', 'http://localhost:8083')]:
                for name, path in [('search', '/?q=bitcoin&mode=relevance'),
                                   ('social', '/?mode=discovery&reach=3&order=connections'),
                                   ('context', context_path)]:
                    requests.clear()
                    response = await page.goto(base + path)
                    assert response.status == 200, (label, name, response.status)
                    await page.evaluate('window.scrollTo(0,0)')
                    await page.screenshot(path=str(OUT / f'{label}-{name}.png'))
                    ids = await page.locator('article.post').evaluate_all('(nodes)=>nodes.map(n=>n.id)')
                    findings.append({'reader': label, 'case': name, 'url': page.url, 'ids': ids,
                                     'requests': list(requests)})
                await page.set_viewport_size({'width': 390, 'height': 844})
                await page.goto(base + '/?q=bitcoin&mode=relevance')
                await page.evaluate('window.scrollTo(0,0)')
                await page.screenshot(path=str(OUT / f'{label}-mobile.png'))
                assert await page.evaluate('document.documentElement.scrollWidth <= innerWidth')
                await page.set_viewport_size({'width': 1280, 'height': 900})
                await page.goto(base + '/?mode=new')
                await page.get_by_role('link', name='older →', exact=True).click()
                assert 'page=1' in page.url
                assert await page.locator('article.post').count() == 50
                findings.append({'reader': label, 'case': 'page-two', 'url': page.url,
                                 'ids': await page.locator('article.post').evaluate_all('(nodes)=>nodes.map(n=>n.id)')})
                await page.locator('#q-input').fill('bitcoin')
                async with page.expect_navigation():
                    await page.locator('button[name=go]').click()
                assert await page.locator('#feed-select').input_value() == 'relevance'
                async with page.expect_navigation():
                    await page.locator('#feed-select').select_option('discovery')
                assert 'mode=discovery' in page.url and 'q=bitcoin' in page.url
                async with page.expect_navigation():
                    await page.locator('#reach-select').select_option('3')
                async with page.expect_navigation():
                    await page.locator('#order-select').select_option('connections')
                assert 'order=connections' in page.url
                details = page.locator('details.rest').first
                await details.locator('summary').click()
                assert await details.get_attribute('open') is not None
                await details.locator('summary').click()
                assert await details.get_attribute('open') is None
                assert await page.locator('article.post img, article.post script, article.post iframe, article.post video, article.post audio').count() == 0
                for path, status in [('/status', 200), ('/about', 200), ('/tos', 200), ('/?q=%22unclosed', 400), ('/context/nostr/absent', 404)]:
                    response = await page.goto(base + path)
                    assert response.status == status, (label, path, response.status)
                findings.append({'reader': label, 'case': 'interactions', 'page_two': 50,
                                 'search_go': 'relevance', 'feed_change': 'discovery', 'reach': 3,
                                 'order': 'connections', 'expand_collapse': True, 'negative_routes': True})
            await context.close()
        (OUT / 'observations.json').write_text(json.dumps(findings, indent=2))
        print(json.dumps({'paired_cases': 4, 'screenshots': 8, 'interactions': 'passed'}, indent=2))

    asyncio.run(capture())
