#!/usr/bin/env python3
"""Erika 图标工坊的本地服务。

    python3 design/brand/icon-studio-server.py [端口，默认 4180]

然后打开 http://127.0.0.1:4180/ （会跳到 brand/icon-studio.html）。

- 把 design/ 整个文件夹当静态网站提供（工坊页面要用 ../assets/ 里的图）。
- 实时同步：正在编辑的设计实时写在 design/brand/icon-designs/_live.json。
  网页上每改一步就写一次；有人直接改这个文件（比如 Claude），所有打开的页面一秒内跟着变。
  多个窗口同时开着也会互相同步。页面代码（icon-studio.html、icon-studio-icons.js）改了，页面自动刷新。
- 工坊里点「保存」时，设计另存一份快照到 design/brand/icon-designs/：
  <名字>.json（形状数据，能再载入）和 <名字>.svg（矢量图），Claude 可以直接读这些文件。
- 只监听本机 127.0.0.1，不对外开放；只用 Python 标准库。
"""
import json
import os
import queue
import re
import sys
import threading
import time
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, unquote, urlparse

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # design/
SAVE_DIR = os.path.join(ROOT, 'brand', 'icon-designs')
PAGE = '/brand/icon-studio.html'
CODE_FILES = [os.path.join(ROOT, 'brand', f) for f in ('icon-studio.html', 'icon-studio-icons.js')]


def safe_name(name):
    """文件名：去掉路径和系统不允许的字符，最长 60 个字。"""
    s = re.sub(r'[\\/:*?"<>|\x00-\x1f]', '', str(name or '')).strip().strip('.')
    return s[:60] or '未命名'


def path_for(stem, ext):
    p = os.path.realpath(os.path.join(SAVE_DIR, stem + ext))
    if not p.startswith(os.path.realpath(SAVE_DIR) + os.sep):
        raise ValueError('bad name')
    return p


def mtime(p):
    try:
        return os.stat(p).st_mtime_ns
    except OSError:
        return None


class Live:
    """一份实时同步的设计（默认 _live，测试时可以用别的名字，比如 _test）。

    rev 每变一次加一；谁改的记在 by 里：网页窗口用自己的随机 id，直接改文件的记成 'file'。"""

    docs = {}
    listeners = []  # 每个打开的页面一个队列：(文档名, 队列)
    lock = threading.Lock()

    def __init__(self, name):
        self.name, self.path = name, path_for(name, '.json')
        self.rev, self.text, self.mtime = 0, None, None
        try:
            with open(self.path, encoding='utf-8') as f:
                self.text = f.read()
            json.loads(self.text)
            self.rev, self.mtime = 1, mtime(self.path)
        except (OSError, ValueError):
            self.text = None

    @classmethod
    def get(cls, name):
        if not re.fullmatch(r'_[a-z0-9]{1,20}', name or ''):
            raise ValueError('bad doc')
        with cls.lock:
            if name not in cls.docs:
                cls.docs[name] = Live(name)
            return cls.docs[name]

    def state(self, by='init'):
        return {'rev': self.rev, 'by': by, 'design': json.loads(self.text) if self.text else None}

    def write(self, design, by):
        text = json.dumps(design, ensure_ascii=False, indent=2) + '\n'
        with Live.lock:
            if text != self.text:
                os.makedirs(SAVE_DIR, exist_ok=True)
                tmp = self.path + '.tmp'
                with open(tmp, 'w', encoding='utf-8') as f:
                    f.write(text)
                os.replace(tmp, self.path)
                self.text, self.mtime = text, mtime(self.path)
                self.rev += 1
            st = self.state(by)
        Live.broadcast(self.name, 'design', st)
        return st['rev']

    def poll(self):
        """文件被别人改了（mtime 变了、内容也变了）就广播出去。"""
        m = mtime(self.path)
        if m is None and self.text is not None:
            # 文件被删了：清空，下一个连上的页面会用它自己的草稿重新建
            with Live.lock:
                self.text, self.mtime = None, None
                self.rev += 1
            sys.stderr.write('[icon-studio] %s.json 被删除了\n' % self.name)
            return
        if m is None or m == self.mtime:
            return
        try:
            with open(self.path, encoding='utf-8') as f:
                text = f.read()
        except OSError:
            return
        self.mtime = m
        if text == self.text:
            return
        try:
            d = json.loads(text)
            if not isinstance(d.get('shapes'), list):
                raise ValueError('shapes')
        except (ValueError, AttributeError) as e:
            Live.broadcast(self.name, 'bad', {'error': str(e)})
            sys.stderr.write('[icon-studio] %s.json 格式不对，没有同步：%s\n' % (self.name, e))
            return
        with Live.lock:
            self.text = text
            self.rev += 1
            st = self.state('file')
        sys.stderr.write('[icon-studio] %s.json 被直接修改，已同步到页面（rev %d）\n' % (self.name, self.rev))
        Live.broadcast(self.name, 'design', st)

    @classmethod
    def broadcast(cls, name, event, data):
        msg = (event, json.dumps(data, ensure_ascii=False))
        with cls.lock:
            targets = [q for (n, q) in cls.listeners if n == name or name is None]
        for q in targets:
            q.put(msg)


def watcher():
    """每 0.3 秒看一次：实时设计文件被直接改了吗？页面代码改了吗？"""
    code = {p: mtime(p) for p in CODE_FILES}
    while True:
        time.sleep(.3)
        for doc in list(Live.docs.values()):
            doc.poll()
        for p in CODE_FILES:
            m = mtime(p)
            if m != code[p]:
                code[p] = m
                Live.broadcast(None, 'code', {'file': os.path.basename(p)})


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=ROOT, **kwargs)

    def end_headers(self):
        self.send_header('Cache-Control', 'no-store')
        super().end_headers()

    def log_message(self, fmt, *args):
        if self.path.startswith('/api/') and '/events' not in self.path and not self.path.startswith('/api/live'):
            sys.stderr.write('[icon-studio] %s\n' % (fmt % args))

    def send_json(self, code, obj):
        body = json.dumps(obj, ensure_ascii=False).encode('utf-8')
        self.send_response(code)
        self.send_header('Content-Type', 'application/json; charset=utf-8')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def api_id(self, path):
        return unquote(path[len('/api/designs/'):])

    def live_doc(self):
        q = parse_qs(urlparse(self.path).query)
        return Live.get((q.get('doc') or ['_live'])[0])

    def read_body(self):
        n = int(self.headers.get('Content-Length') or 0)
        return json.loads(self.rfile.read(n).decode('utf-8'))

    def do_GET(self):
        path = urlparse(self.path).path
        if path == '/':
            self.send_response(302)
            self.send_header('Location', PAGE)
            self.end_headers()
            return
        if path == '/api/live':
            try:
                return self.send_json(200, self.live_doc().state())
            except ValueError as e:
                return self.send_json(400, {'error': str(e)})
        if path == '/api/live/events':
            return self.events()
        if path == '/api/designs':
            items = []
            if os.path.isdir(SAVE_DIR):
                for fn in os.listdir(SAVE_DIR):
                    if not fn.endswith('.json') or fn.startswith('_'):
                        continue
                    try:
                        with open(os.path.join(SAVE_DIR, fn), encoding='utf-8') as f:
                            d = json.load(f)
                        items.append({'id': fn[:-5], 'name': d.get('name') or fn[:-5], 'updatedAt': d.get('updatedAt', 0)})
                    except (OSError, ValueError):
                        continue
            items.sort(key=lambda x: x['updatedAt'], reverse=True)
            return self.send_json(200, items)
        if path.startswith('/api/designs/'):
            try:
                with open(path_for(self.api_id(path), '.json'), encoding='utf-8') as f:
                    return self.send_json(200, json.load(f))
            except (OSError, ValueError):
                return self.send_json(404, {'error': 'not found'})
        return super().do_GET()

    def events(self):
        """Server-Sent Events：一连上就发当前设计，之后有变化就推。"""
        try:
            doc = self.live_doc()
        except ValueError as e:
            return self.send_json(400, {'error': str(e)})
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream; charset=utf-8')
        self.end_headers()
        q = queue.Queue()
        with Live.lock:
            Live.listeners.append((doc.name, q))
        q.put(('design', json.dumps(doc.state(), ensure_ascii=False)))
        try:
            while True:
                try:
                    event, data = q.get(timeout=15)
                    self.wfile.write(('event: %s\ndata: %s\n\n' % (event, data)).encode('utf-8'))
                except queue.Empty:
                    self.wfile.write(b': ping\n\n')
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError, OSError):
            pass
        finally:
            with Live.lock:
                Live.listeners.remove((doc.name, q))

    def do_POST(self):
        path = urlparse(self.path).path
        if path == '/api/live':
            try:
                body = self.read_body()
                design = body.get('design') or {}
                if not isinstance(design.get('shapes'), list):
                    raise ValueError('shapes')
                return self.send_json(200, {'rev': self.live_doc().write(design, str(body.get('client') or 'page'))})
            except (ValueError, OSError) as e:
                return self.send_json(400, {'error': str(e)})
        if path != '/api/designs':
            return self.send_json(404, {'error': 'not found'})
        try:
            body = self.read_body()
            design = body.get('design') or {}
            shapes = design.get('shapes')
            if not isinstance(shapes, list):
                raise ValueError('shapes')
            name = safe_name(body.get('name'))
            if name.startswith('_'):
                name = name.lstrip('_') or '未命名'
            stem = name
            os.makedirs(SAVE_DIR, exist_ok=True)
            if body.get('asNew'):
                i = 2
                while os.path.exists(path_for(stem, '.json')):
                    stem = '%s %d' % (name, i)
                    i += 1
            # 设计里的字段（bg、shapes、blend、mask……）原样存下，以后工坊加新字段不用再改这里
            doc = {k: v for k, v in design.items() if k not in ('name', 'updatedAt')}
            doc.update({'name': stem, 'shapes': shapes, 'updatedAt': int(time.time() * 1000)})
            with open(path_for(stem, '.json'), 'w', encoding='utf-8') as f:
                json.dump(doc, f, ensure_ascii=False, indent=2)
            svg = body.get('svg')
            if isinstance(svg, str) and svg.startswith('<svg'):
                with open(path_for(stem, '.svg'), 'w', encoding='utf-8') as f:
                    f.write(svg + '\n')
            return self.send_json(200, {'id': stem})
        except (ValueError, OSError) as e:
            return self.send_json(400, {'error': str(e)})

    def do_DELETE(self):
        path = urlparse(self.path).path
        if not path.startswith('/api/designs/'):
            return self.send_json(404, {'error': 'not found'})
        try:
            stem = self.api_id(path)
            if stem.startswith('_'):
                raise ValueError('live doc')
            for ext in ('.json', '.svg'):
                p = path_for(stem, ext)
                if os.path.exists(p):
                    os.remove(p)
            return self.send_json(200, {'ok': True})
        except (ValueError, OSError) as e:
            return self.send_json(400, {'error': str(e)})


def main():
    port = int(os.environ.get('PORT') or (sys.argv[1] if len(sys.argv) > 1 else 4180))
    server = ThreadingHTTPServer(('127.0.0.1', port), Handler)
    server.daemon_threads = True
    Live.get('_live')
    threading.Thread(target=watcher, daemon=True).start()
    print('Erika 图标工坊：http://127.0.0.1:%d/  （实时设计：%s）' % (port, Live.get('_live').path), flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass


if __name__ == '__main__':
    main()
