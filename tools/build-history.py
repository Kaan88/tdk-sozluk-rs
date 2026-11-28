r"""Build data/history.sqlite: how the entries of TDK's Güncel Türkçe Sözlük read in earlier years, for the
notes Sözlük shows under an entry ("Meaning 3 was added after 2019", "Written “yeşilbiber” until 2019").
The history is never mixed into gts.sqlite, which holds only TDK's current entries.

usage: python -I -X utf8 tools/build-history.py [--out PATH] [--cache DIR] [--no-wayback] [--rate R]
                                                [--per-year N] [--limit N]

Sources, each kept in --cache (default data/history-src/) once downloaded, so a run that stops goes on where
it left off:
  - Full copies of the dictionary:
      2007        TDK's 2007 edition (its desktop program's database, as SQLite), 73,707 headwords
      2019-06-06  the 11th edition as served online, 92,406 headwords
      2023-07-23  the dump Sözlük's own database was first seeded from, 98,714 headwords
      2024-03-29  the 12th edition as served online, 99,236 headwords
    The 2007, 2019 and 2024 copies are from github.com/ogun/guncel-turkce-sozluk, which was deleted; they are
    taken from the fork sonsuzus/guncel-turkce-sozluk at a fixed commit (Software Heritage also keeps the
    original: archive.softwareheritage.org, origin https://github.com/ogun/guncel-turkce-sozluk). The 2023
    copy is from anezih/guncel-turkce-sozluk-kindle-kobo-stardict. Each file is checked by SHA-256.
  - TDK's headword list (sozluk.gov.tr/autocomplete.json) as the Wayback Machine captured it, 2019 to 2026:
    only words, but it dates spelling changes more closely.
  - TDK's word pages as the Wayback Machine captured them: the 2009-2011 site (SozBul.aspx) and the
    2012-2019 site (index.php?option=com_gts), about 50,000 captures. At most --per-year captures of one
    word in one year are fetched (default 1), at --rate requests a second (default 1; the Wayback Machine
    asks for gentle use). That part takes many hours; --no-wayback leaves it out, and --limit N stops after
    N pages (to try it).

The result (--out, default data/history.sqlite):
  versions(word, first, last, sources, meanings)  one row per form an entry had: the dates it was first and
      last seen in that form, where it was seen (comma-separated), and its meanings as a JSON list of the
      texts, in TDK's order. A form that only differs in letter case, punctuation, circumflexes or a
      "►"/"bk." cross-reference mark counts as the same form.
  spellings(old, new, last_old, first_new)  a headword that disappeared while one written differently
      appeared (joined, split, hyphenated, a letter changed): "yeşilbiber" -> "yeşil biber".
  meta(key, value)  when it was built and from what.
Copy it next to gts.sqlite (%LOCALAPPDATA%\sozluk) for the app; Sözlük reads it read-only.
"""
import gzip, hashlib, html, io, json, os, re, sqlite3, sys, tarfile, time, unicodedata, urllib.parse
import urllib.error, urllib.request, zlib

HERE = os.path.dirname(os.path.abspath(__file__))

def opt(name, default):
    return type(default)(sys.argv[sys.argv.index(name) + 1]) if name in sys.argv else default
OUT = opt('--out', os.path.join(HERE, '..', 'data', 'history.sqlite'))
CACHE = opt('--cache', os.path.join(HERE, '..', 'data', 'history-src'))
RATE, PER_YEAR, LIMIT = opt('--rate', 1.0), opt('--per-year', 1), opt('--limit', 0)
WAYBACK = '--no-wayback' not in sys.argv
UA = 'Sozluk-history/1 (+https://github.com/Kaan88/tdk-sozluk-rs)'

OGUN = 'https://raw.githubusercontent.com/sonsuzus/guncel-turkce-sozluk/b0fcfec6d3cb6fc9fe34b53e21cd11f84fc87fbe/'
DUMPS = [  # (date, file name, URL, SHA-256, kind)
    ('2007', 'gts-2007.sqlite', OGUN + 'sozluk/backup-2007/backup-2007.sqlite3.db',
     'fd28288df018550e44261b7c6512bf9123c1aa081dcc09d7e1cc88728469da59', 'tdk2007'),
    ('2019-06-06', 'gts-2019.json.tar.gz', OGUN + 'sozluk/v11/v11.gts.json.tar.gz',
     'bfa76f3653315f110e7e2b592b958838f84dd3b06ae83b344ebdf01adff0dc61', 'gts'),
    ('2023-07-23', 'gts-2023.json.tar.gz', 'https://raw.githubusercontent.com/anezih/'
     'guncel-turkce-sozluk-kindle-kobo-stardict/bc94987d1e3524a8acfcc245d84c31a95f2233f2/src/gts.json.tar.gz',
     '08a549ed5a4c8c116d0eac16e029c282593191abd9a5d2dcc2af598a3815090e', 'gts'),
    ('2024-03-29', 'gts-2024.json.tar.gz', OGUN + 'sozluk/v12/v12.gts.json.tar.gz',
     '6935628850a8827e51db6813ce8c702ea1a65f6b56258c8d9ad97547dae5b20d', 'gts'),
]

def log(*a):
    print(time.strftime('%H:%M:%S'), *a, flush=True)

# ---- text --------------------------------------------------------------------------------------------------

def lower(s):
    return s.replace('I', 'ı').replace('İ', 'i').lower()

def plain(s):
    """A meaning as compared: no cross-reference mark, case, punctuation or circumflex."""
    s = lower(s.replace('►', ' ').replace('bk.', ' '))
    s = ''.join(c for c in unicodedata.normalize('NFD', s) if unicodedata.category(c) != 'Mn')
    return ' '.join(re.sub(r'[^\w]+', ' ', s).split())

def fold(w):
    """A headword as compared for spelling changes: no case, circumflex, space, hyphen or apostrophe."""
    w = ''.join(c for c in unicodedata.normalize('NFD', lower(w)) if unicodedata.category(c) != 'Mn')
    return re.sub(r"[\s\-'’]+", '', w)

def same_spelling(a, b):
    """Two headwords that only differ in spaces at the ends or in the apostrophe used (’ or ')."""
    return a.strip().replace('’', "'") == b.strip().replace('’', "'")

def same_form(a, b):
    return sorted(map(plain, a)) == sorted(map(plain, b))

# ---- downloads ---------------------------------------------------------------------------------------------

def get(url, tries=8):
    """The body of url. Waits and tries again on a refusal or a server that is down (the Wayback Machine
    answers 429 and 503 when it wants fewer requests, and is sometimes offline for a while)."""
    wait = 30
    for k in range(tries):
        try:
            req = urllib.request.Request(url, headers={'User-Agent': UA})
            with urllib.request.urlopen(req, timeout=120) as r:
                body = r.read()
            if body[:2] == b'\x1f\x8b':  # the Wayback Machine gives some captures as they were sent, gzip'd
                body = gzip.decompress(body)
            if b'Temporarily Offline' in body[:3000] and b'Internet Archive' in body[:3000]:
                raise OSError('the Wayback Machine is temporarily offline')
            return body
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            if e.code not in (429, 500, 502, 503, 504):
                raise
            why = f'HTTP {e.code}'
        except OSError as e:
            why = str(e)
        if k == tries - 1:
            raise OSError(f'{url}: {why}')
        log(f'  {why}; waiting {wait} s')
        time.sleep(wait)
        wait = min(wait * 2, 900)

def fetch_file(name, url, sha):
    path = os.path.join(CACHE, name)
    if not os.path.exists(path):
        log('downloading', name)
        body = get(url)
        if body is None:
            sys.exit(f'{url}: not found (the source repository may be gone; see the header for Software Heritage)')
        if hashlib.sha256(body).hexdigest() != sha:
            sys.exit(f'{url}: SHA-256 differs from the one this script expects; not used')
        with open(path + '.tmp', 'wb') as f:
            f.write(body)
        os.replace(path + '.tmp', path)
    return path

# ---- full copies -------------------------------------------------------------------------------------------

def read_gts(path):
    """{headword: [meaning, ...]} from a JSON list or JSON Lines of TDK /gts entries, in a .tar.gz."""
    t = tarfile.open(path)
    raw = t.extractfile([m for m in t.getmembers() if m.isfile()][0]).read().decode('utf-8').strip()
    items = json.loads(raw) if raw.startswith('[') else (json.loads(l) for l in raw.splitlines() if l.strip())
    out = {}
    for e in sorted(items, key=lambda e: int(e.get('madde_id') or 0)):
        w = (e.get('madde') or '').strip()
        if w:
            out.setdefault(w, []).extend(m['anlam'].strip() for m in e.get('anlamlarListe') or [] if m.get('anlam'))
    return out

def read_2007(path):
    db = sqlite3.connect(f'file:{path}?mode=ro', uri=True)
    out = {}
    for w, meaning in db.execute(
            'SELECT m.HEAD_MULT, s.MEANING FROM WORDMULT m LEFT JOIN SENSES s '
            'ON s.WORD_ID = m.WORD_ID AND s.MULT_NO = m.MULT_NO '
            'ORDER BY CAST(m.WORD_ID AS INTEGER), CAST(m.MULT_NO AS INTEGER), s.SENSE_NO'):
        w = (w or '').strip()
        if w:
            out.setdefault(w, [])
            if meaning and meaning.strip():
                out[w].append(meaning.strip())
    return out

# ---- the Wayback Machine -----------------------------------------------------------------------------------

CDX = 'https://web.archive.org/cdx/search/cdx?'

def cdx(params):
    body = get(CDX + urllib.parse.urlencode(params, doseq=True))
    return [l.split(' ') for l in (body or b'').decode('utf-8', 'replace').splitlines() if l.strip()]

def headword_lists():
    """[(date, {headword})] from the captures of sozluk.gov.tr/autocomplete.json."""
    out = []
    for ts, digest in cdx({'url': 'sozluk.gov.tr/autocomplete.json', 'fl': 'timestamp,digest',
                           'filter': 'statuscode:200', 'collapse': 'digest'}):
        path = os.path.join(CACHE, f'autocomplete-{ts}.json')
        if not os.path.exists(path):
            body = get(f'https://web.archive.org/web/{ts}id_/https://sozluk.gov.tr/autocomplete.json')
            with open(path, 'wb') as f:
                f.write(body or b'')
            time.sleep(1 / RATE)
        try:
            raw = open(path, 'rb').read()
            raw = gzip.decompress(raw) if raw[:2] == b'\x1f\x8b' else raw
            words = {x['madde'].strip() for x in json.loads(raw.decode('utf-8')) if x.get('madde')}
        except (ValueError, TypeError, KeyError, AttributeError):
            continue  # a capture that is not the list (an error page)
        if len(words) > 50000:
            out.append((f'{ts[:4]}-{ts[4:6]}-{ts[6:8]}', words))
    return out

def page_list():
    """[(timestamp, url, word)] of the word pages to fetch, at most PER_YEAR a word a year."""
    rows = []
    for params, era in (
            ({'url': 'tdk.gov.tr/index.php', 'matchType': 'prefix', 'filter': ['statuscode:200', 'original:.*com_gts.*']},
             'gts'),
            ({'url': 'tdk.gov.tr/TR/Genel/SozBul.aspx', 'matchType': 'prefix', 'filter': 'statuscode:200'}, 'sozbul')):
        params = dict(params, fl='timestamp,original', to='2019')
        for ts, url in cdx(params):
            q = urllib.parse.parse_qs(urllib.parse.urlsplit(html.unescape(url)).query.replace('%20', '+'))
            k = {key.strip().lower(): v for key, v in q.items()}
            word = (k.get('kelime') or k.get('amp;kelime') or [''])[0].strip()
            if word:
                rows.append((ts, url, word))
    seen, keep = {}, []
    for ts, url, word in sorted(rows):
        key = (lower(word), ts[:4])
        if seen.get(key, 0) < PER_YEAR:
            seen[key] = seen.get(key, 0) + 1
            keep.append((ts, url, word))
    return keep

def text(fragment):
    t = re.sub(r'(?is)<[^>]+>', ' ', fragment)
    return ' '.join(html.unescape(t).replace('\xa0', ' ').split())

def headword(fragment):
    """A page's headword, without the number a homograph has after it: "zira (I)" is "zira"."""
    return re.sub(r'\s*\([IVX]+\)$', '', text(fragment))

def meaning(fragment):
    """A meaning's text, without the colon that leads into its example."""
    return text(fragment).rstrip(':').strip()

def parse_page(body):
    """{headword: [meaning, ...]} from one captured word page of either site, or {} when it holds none (the
    2009-2010 site loaded some answers by script, so their captures hold only the search form)."""
    t = body.decode('utf-8', 'replace')
    out = {}
    # 2012-2019: one table id='hor-minimalist-a' an entry; the headword in <thead>, a meaning a row.
    for table in re.findall(r"(?is)<table[^>]*id=['\"]?hor-minimalist-a['\"]?[^>]*>(.*?)</table>", t):
        head = re.search(r'(?is)<thead>.*?<b>(.*?)</b>', table)
        if not head:
            continue
        w = headword(head.group(1))
        body_ = table[head.end():]
        for cell in re.findall(r'(?is)<td>(.*?)</td>', body_):
            first = re.split(r'(?i)<br\s*/?>', cell, maxsplit=1)[0]
            first = re.sub(r'(?is)^\s*\d+\.\s*', '', first)
            first = re.sub(r'(?is)^\s*<i>.*?</i>', '', first)  # the word class
            m = meaning(first)
            if m:
                out.setdefault(w, []).append(m)
    if out:
        return out
    # 2009-2011, in div SorguSonucu. An entry is a centred paragraph (the headword in red; a homograph's
    # number and the inflection in dark blue tags around it) and a table after it, a meaning a cell.
    res = re.search(r'(?is)<div id="ctl00_ContentPlaceHolder1_SorguSonucu">(.*?)</div>', t)
    if res:
        part = re.sub(r'(?is)<FONT color=DarkBlue>.*?</FONT>', '', res.group(1))
        for block in re.split(r'(?i)<P ALIGN=center>', part)[1:]:
            head = re.search(r'(?is)<FONT color=crimson>(.*?)</FONT>', block)
            if not head:
                continue
            w = headword(head.group(1))
            for cell in re.findall(r'(?is)<TD>(.*?)</TD>', block):
                first = re.split(r'(?i)<br\s*/?>', cell, maxsplit=1)[0]
                first = re.sub(r'(?is)<B>.*?</B>', '', first, count=1).replace('&nbsp;', ' ')
                first = re.sub(r'(?is)^\s*(<I>.*?</I>\s*)+', '', first)  # the word class: "sıfat, mecaz"
                m = meaning(first)
                if w and m:
                    out.setdefault(w, []).append(m)
    return out

def wayback_pages(cache):
    """{headword: [(date, [meaning, ...])]} from the captured word pages, fetching what the cache lacks."""
    cache.execute('CREATE TABLE IF NOT EXISTS pages(ts TEXT, url TEXT, word TEXT, body BLOB, PRIMARY KEY(ts, url))')
    done = {(ts, u) for ts, u in cache.execute('SELECT ts, url FROM pages')}
    todo = [r for r in page_list() if (r[0], r[1]) not in done]
    if LIMIT:
        todo = todo[:LIMIT]
    log(f'word pages: {len(done)} in the cache, {len(todo)} to fetch at {RATE}/s '
        f'(about {len(todo) / RATE / 3600:.1f} h)')
    for n, (ts, url, word) in enumerate(todo, 1):
        body = get(f'https://web.archive.org/web/{ts}id_/{url}')
        cache.execute('INSERT OR REPLACE INTO pages VALUES (?,?,?,?)', (ts, url, word, zlib.compress(body or b'', 9)))
        if n % 50 == 0 or n == len(todo):
            cache.commit()
            log(f'  {n}/{len(todo)}')
        time.sleep(1 / RATE)
    cache.commit()
    out, pages, found = {}, 0, 0
    for ts, body in cache.execute('SELECT ts, body FROM pages ORDER BY ts'):
        pages += 1
        got = parse_page(zlib.decompress(body))
        found += bool(got)
        for w, ms in got.items():
            out.setdefault(w, []).append((f'{ts[:4]}-{ts[4:6]}-{ts[6:8]}', ms))
    log(f'word pages: {pages} in the cache, {found} with an entry, {len(out)} headwords')
    return out

# ---- build -------------------------------------------------------------------------------------------------

def main():
    os.makedirs(CACHE, exist_ok=True)
    cache = sqlite3.connect(os.path.join(CACHE, 'wayback.sqlite'))
    seen = {}       # headword -> [(date, source, [meaning])]
    sets = []       # (date, {headword}) of every full copy and headword list, for the spelling changes
    for date, name, url, sha, kind in DUMPS:
        path = fetch_file(name, url, sha)
        d = read_2007(path) if kind == 'tdk2007' else read_gts(path)
        log(f'{date}: {len(d)} headwords')
        for w, ms in d.items():
            seen.setdefault(w, []).append((date, f'copy {date[:4]}', ms))
        sets.append((date, set(d)))
    if WAYBACK:
        lists = headword_lists()
        log(f'headword lists: {len(lists)} dated captures, {lists[0][0] if lists else "-"} to {lists[-1][0] if lists else "-"}')
        sets += lists
        for w, caps in wayback_pages(cache).items():
            for date, ms in caps:
                seen.setdefault(w, []).append((date, 'Wayback', ms))

    if os.path.exists(OUT + '.tmp'):
        os.remove(OUT + '.tmp')
    out = sqlite3.connect(OUT + '.tmp')
    out.executescript('''
        CREATE TABLE versions(word TEXT NOT NULL, first TEXT NOT NULL, last TEXT NOT NULL, sources TEXT NOT NULL,
                              meanings TEXT NOT NULL);
        CREATE INDEX versions_word ON versions(word);
        CREATE TABLE spellings(old TEXT NOT NULL, new TEXT NOT NULL, last_old TEXT NOT NULL, first_new TEXT NOT NULL);
        CREATE INDEX spellings_old ON spellings(old);
        CREATE INDEX spellings_new ON spellings(new);
        CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT);
    ''')
    n_versions = 0
    for w, rows in seen.items():
        forms = []  # [first, last, sources, meanings]
        for date, source, ms in sorted(rows, key=lambda r: r[0]):
            if not ms:
                continue
            if forms and same_form(forms[-1][3], ms):
                forms[-1][1] = date
                if source not in forms[-1][2]:
                    forms[-1][2].append(source)
            else:
                forms.append([date, date, [source], ms])
        for first, last, sources, ms in forms:
            out.execute('INSERT INTO versions VALUES (?,?,?,?,?)',
                        (w, first, last, ','.join(sources), json.dumps(ms, ensure_ascii=False)))
        n_versions += len(forms)

    sets.sort(key=lambda s: s[0])
    pairs = {}
    for (d0, a), (d1, b) in zip(sets, sets[1:]):
        gone, came = a - b, b - a
        by_fold = {}
        for w in came:
            by_fold.setdefault(fold(w), []).append(w)
        for old in gone:
            for new in by_fold.get(fold(old), []):
                if same_spelling(old, new) or (old, new) in pairs:
                    continue
                pairs[(old, new)] = (d0, d1)
    # A spelling that went one way and came back is a slip in one list, not a change.
    pairs = {k: v for k, v in pairs.items() if (k[1], k[0]) not in pairs}
    for (old, new), (d0, d1) in sorted(pairs.items()):
        out.execute('INSERT INTO spellings VALUES (?,?,?,?)', (old.strip(), new.strip(), d0, d1))

    meta = {'built': time.strftime('%Y-%m-%d %H:%M:%S'), 'copies': ', '.join(d[0] for d in DUMPS),
            'headword_lists': str(len(sets) - len(DUMPS)), 'wayback': 'yes' if WAYBACK else 'no'}
    out.executemany('INSERT INTO meta VALUES (?,?)', meta.items())
    out.commit()
    out.execute('VACUUM')
    out.close()
    os.replace(OUT + '.tmp', OUT)
    log(f'{OUT}: {len(seen)} headwords, {n_versions} versions, {len(pairs)} spelling changes '
        f'({os.path.getsize(OUT) / 1e6:.1f} MB)')

if __name__ == '__main__':
    main()
