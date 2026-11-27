r"""Build or refresh the full offline TDK Güncel Türkçe Sözlük that Sözlük reads: data/gts.sqlite.

usage: python -I -X utf8 tools/tdk-crawl.py [--db PATH] [--rate 1.5] [--workers 3] [--retry-missing] [--limit N]
                                            [--shard i/n]
       python -I -X utf8 tools/tdk-crawl.py [--db PATH] --merge OTHER.sqlite [...]
The database is data/gts.sqlite in this repository unless --db names another (it is not committed); copy it to
%LOCALAPPDATA%\sozluk\gts.sqlite or next to sozluk.exe for the app. --merge adds the found and not-found answers of
other runs (shards) that this database lacks, then stops.
For every headword in data/gts-headwords.txt, fetches sozluk.gov.tr/gts?ara=<word> and stores the raw JSON
answer (zlib) in table gts(word PRIMARY KEY, status, data, fetched). status: ok | notfound | error.
Resumable: words already stored as ok/notfound are skipped (errors are retried on every run; --retry-missing also
retries notfound). Pace: starts at --rate requests/s (default 1.5). HTTP 429 (TDK's rate limit): everyone stops for
Retry-After or 90 s, then the ceiling drops to 70% of the rate that was hit (never under 0.3/s), and the crawl creeps
back up 1% per success to that ceiling; the ceiling itself climbs 0.3% per success back toward --rate, so one bad
minute does not cap the whole crawl. Timeouts and dropped connections (TDK also limits by stalling them) count
the same as a 429: a pause of 60 s, doubling while the stall lasts (up to 15 min), and the same lower ceiling. Other
errors or replies slower than 5 s: pace x0.7; after 20 of them in a row it waits 5 minutes. A 429'd or stalled word is
retried, not stored as an error. Each worker keeps one connection open (keep-alive), as a browser does.
--shard i/n takes every n-th headword from the i-th (0-based): run 0/3, 1/3, 2/3 on three machines (three IPs) and
merge their gts.sqlite files afterwards; each machine's limit is its own. Saved every 30 s and at exit; Ctrl+C or SIGTERM stops it within seconds (a request in flight may take up to 20 s).
Progress line every 200 words on stdout.
"""
import http.client, json, os, signal, socket, sqlite3, ssl, sys, threading, time, urllib.parse, zlib
from concurrent.futures import ThreadPoolExecutor

HERE = os.path.dirname(os.path.abspath(__file__))
LIST = os.path.join(HERE, '..', 'data', 'gts-headwords.txt')

def opt(name, default):
    return type(default)(sys.argv[sys.argv.index(name) + 1]) if name in sys.argv else default
RATE, WORKERS, LIMIT, SHARD = opt('--rate', 1.5), opt('--workers', 3), opt('--limit', 0), opt('--shard', '0/1')
DB = opt('--db', os.path.join(HERE, '..', 'data', 'gts.sqlite'))

db = sqlite3.connect(DB, check_same_thread=False)
db.execute('CREATE TABLE IF NOT EXISTS gts(word TEXT PRIMARY KEY, status TEXT, data BLOB, fetched TEXT)')
db.commit()
if '--merge' in sys.argv:
    for other in sys.argv[sys.argv.index('--merge') + 1:]:
        if other.startswith('--'):
            break
        before = db.total_changes
        db.execute('ATTACH DATABASE ? AS o', (other,))
        db.execute("INSERT OR REPLACE INTO gts SELECT * FROM o.gts WHERE status IN ('ok', 'notfound') "
                   "AND word NOT IN (SELECT word FROM main.gts WHERE status IN ('ok', 'notfound'))")
        db.commit()
        db.execute('DETACH DATABASE o')
        print(f'{other}: {db.total_changes - before} entries added', flush=True)
    print('database:', dict(db.execute('SELECT status, COUNT(*) FROM gts GROUP BY status').fetchall()), flush=True)
    sys.exit(0)
lock = threading.Lock()
skip = ('ok',) if '--retry-missing' in sys.argv else ('ok', 'notfound')
done = {w for (w,) in db.execute(f'SELECT word FROM gts WHERE status IN ({",".join("?" * len(skip))})', skip)}
words = [l.rstrip('\n') for l in open(LIST, encoding='utf-8') if l.strip() and not l.startswith('#')]
si, sn = (int(x) for x in SHARD.split('/'))
todo = [w for k, w in enumerate(words) if k % sn == si and w not in done][:LIMIT or None]
print(f'{len(words)} headwords, {len(done)} stored, {len(todo)} to fetch at up to {RATE}/s', flush=True)

pace = {'rate': RATE, 'ceil': RATE, 'next': time.monotonic(), 'errs': 0, 'saved': time.monotonic(), 'stall': 0, 'start': time.monotonic()}
stats = {'ok': 0, 'notfound': 0, 'error': 0}

stop = threading.Event()  # Ctrl+C or SIGTERM: workers leave at their next wait

class Stopped(Exception):
    pass

def slot():
    with lock:
        now = time.monotonic()
        t = max(now, pace['next'])
        pace['next'] = t + 1 / pace['rate']
    if stop.wait(max(0, t - now)):
        raise Stopped

def limited(wait, why='429'):
    with lock:
        if time.monotonic() + 1 < pace['next']:  # another worker already paused everyone
            return
        if why != '429':  # a stall: 60 s, doubling while it lasts
            wait = min(60 * 2 ** pace['stall'], 900)
            pace['stall'] += 1
        pace['ceil'] = max(0.3, pace['rate'] * 0.7)
        pace['rate'] = pace['ceil']
        pace['next'] = time.monotonic() + wait
        print(f'TDK rate limit ({why}): pausing {wait:.0f} s, then {pace["ceil"]:.2f}/s', flush=True)

def feedback(good, why=''):
    with lock:
        if good:
            pace['errs'] = 0
            pace['stall'] = 0
            pace['ceil'] = min(RATE, pace['ceil'] * 1.003)
            pace['rate'] = min(pace['ceil'], pace['rate'] * 1.01)
        else:
            pace['errs'] += 1
            pace['rate'] = max(0.3, pace['rate'] * 0.7)
            if pace['errs'] >= 20:
                print(f'20 errors in a row ({why[:120]}): pausing 5 minutes', flush=True)
                pace['next'] = time.monotonic() + 300
                pace['errs'] = 0

def fetch(w):
    for _ in range(10):
        if stop.is_set():
            return
        try:
            r = fetch_once(w)
        except Stopped:
            return
        if r is not None:
            return
    with lock:
        stats['error'] += 1  # still limited after 10 tries: left for the next run

local = threading.local()

def conn():
    if getattr(local, 'c', None) is None:
        local.c = http.client.HTTPSConnection('sozluk.gov.tr', timeout=20, context=ssl.create_default_context())
    return local.c

def drop():
    try:
        local.c.close()
    except Exception:
        pass
    local.c = None

def fetch_once(w):
    slot()
    path = '/gts?ara=' + urllib.parse.quote(w)
    t0 = time.monotonic()
    try:
        c = conn()
        c.request('GET', path, headers={'User-Agent': 'Mozilla/5.0', 'Accept': 'application/json',
                                        'Connection': 'keep-alive'})
        r = c.getresponse()
        body = r.read()
        if r.getheader('Connection', '').lower() == 'close':
            drop()
        if r.status == 429:
            try:
                wait = float(r.getheader('Retry-After') or 90)
            except ValueError:
                wait = 90.0
            limited(min(max(wait, 30), 900))
            return None
        if r.status != 200:
            raise ValueError(f'HTTP {r.status}: {body[:200]!r}')
        data = json.loads(body.decode('utf-8'))
        status = 'notfound' if isinstance(data, dict) else 'ok'
        feedback(time.monotonic() - t0 < 5)
    except (socket.timeout, TimeoutError, ConnectionError, http.client.RemoteDisconnected,
            http.client.IncompleteRead, ssl.SSLError, OSError) as e:
        drop()
        limited(0, f'stalled: {type(e).__name__}')
        return None
    except Exception as e:
        drop()
        status, body = 'error', f'{type(e).__name__}: {e}'.encode()
        feedback(False, body.decode())
    with lock:
        db.execute('INSERT OR REPLACE INTO gts VALUES (?,?,?,?)',
                   (w, status, zlib.compress(body, 9), time.strftime('%Y-%m-%d %H:%M:%S')))
        stats[status] += 1
        n = sum(stats.values())
        if time.monotonic() - pace['saved'] > 30:
            db.commit()
            pace['saved'] = time.monotonic()
        if n % 200 == 0 or n == len(todo):
            print(f'{n}/{len(todo)} ok {stats["ok"]} notfound {stats["notfound"]} error {stats["error"]} '
                  f'rate {pace["rate"]:.2f}/s, {3600 * n / (time.monotonic() - pace["start"] + 1e-9):.0f} words/h overall',
                  flush=True)
    return True

signal.signal(signal.SIGTERM, lambda *_: stop.set())
ex = ThreadPoolExecutor(WORKERS)
try:
    list(ex.map(fetch, todo))
except KeyboardInterrupt:
    stop.set()
finally:
    ex.shutdown(wait=True, cancel_futures=True)
    if stop.is_set():
        print('stopped; progress saved', flush=True)
    with lock:
        db.commit()
    total = dict(db.execute('SELECT status, COUNT(*) FROM gts GROUP BY status').fetchall())
    print('database:', total, flush=True)
    db.close()
