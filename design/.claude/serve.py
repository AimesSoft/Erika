# 设计文档的预览服务器：和 python3 -m http.server 一样，只是每个响应都带 Cache-Control: no-store，
# 改完文件刷新一下就是新的（普通的 http.server 不发缓存头，浏览器会按启发式规则继续用旧的 CSS / JS / 图片）
import functools
import http.server
import os
import sys

port = int(sys.argv[1]) if len(sys.argv) > 1 else 4179
root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


class NoStore(http.server.SimpleHTTPRequestHandler):
    def end_headers(self):
        self.send_header('Cache-Control', 'no-store')
        super().end_headers()


http.server.ThreadingHTTPServer(('127.0.0.1', port), functools.partial(NoStore, directory=root)).serve_forever()
