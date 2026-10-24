"""Bangumi images through the local proxy, with authenticated DNS and TLS.

Some upstream proxy resolvers return unusable addresses for bgm.tv. Resolve via
HTTPS and tunnel to each returned address, retaining the original TLS server
name and certificate verification. This changes no system DNS configuration.
"""
import http.client
import ipaddress
import json
import ssl
import threading
import time
from urllib.parse import urlsplit, quote


class Tunnel(http.client.HTTPSConnection):
    def __init__(self, proxy, host, address, timeout=12):
        super().__init__(proxy.hostname, proxy.port or 80, timeout=timeout, context=ssl.create_default_context())
        self.target_host = host
        self.set_tunnel(address, 443)

    def connect(self):
        http.client.HTTPConnection.connect(self)
        self.sock = self._context.wrap_socket(self.sock, server_hostname=self.target_host)


class BangumiImages:
    # Bootstrap HTTPS DNS by address, avoiding the proxy's own polluted resolver.
    # TLS still verifies the resolver's hostname with the system trust store.
    resolvers = (
        ("dns.google", "8.8.8.8", "/resolve"),
        ("dns.google", "8.8.4.4", "/resolve"),
        ("cloudflare-dns.com", "1.1.1.1", "/dns-query"),
    )

    def __init__(self, proxy):
        self.proxy = urlsplit(proxy)
        if self.proxy.scheme != "http" or self.proxy.hostname not in {"127.0.0.1", "localhost"}:
            raise ValueError("image DNS transport requires a local HTTP proxy")
        self.addresses = {}
        self.preferred = {}
        self.lock = threading.Lock()

    def resolve(self, host):
        with self.lock:
            old = self.addresses.get(host)
            if old and old[0] > time.time():
                return old[1]
            last = None
            for resolver_host, address, path in self.resolvers:
                conn = Tunnel(self.proxy, resolver_host, address, timeout=8)
                try:
                    conn.request("GET", path + "?name=" + quote(host) + "&type=A",
                                 headers={"Host": resolver_host, "Accept": "application/dns-json"})
                    response = conn.getresponse()
                    if response.status != 200:
                        raise ValueError("DNS service unavailable")
                    body = json.loads(response.read(65536))
                    if body.get("Status") != 0:
                        raise ValueError("DNS lookup failed")
                    answers = [row for row in body.get("Answer", []) if row.get("type") == 1]
                    addresses = [str(ipaddress.IPv4Address(row["data"])) for row in answers
                                 if ipaddress.IPv4Address(row["data"]).is_global]
                    if not addresses:
                        raise ValueError("no public image host address")
                    ttl = max(30, min(300, min(row.get("TTL", 60) for row in answers)))
                    self.addresses[host] = (time.time() + ttl, addresses)
                    return addresses
                except Exception as error:
                    last = error
                finally:
                    conn.close()
            raise OSError("image DNS unavailable") from last

    def get(self, url):
        parts = urlsplit(url)
        if parts.scheme != "https" or parts.hostname not in {"api.bgm.tv", "lain.bgm.tv"}:
            raise ValueError("unsupported image host")
        last = None
        addresses = self.resolve(parts.hostname)
        with self.lock:
            preferred = self.preferred.get(parts.hostname)
        for address in sorted(addresses, key=lambda ip: ip != preferred):
            conn = Tunnel(self.proxy, parts.hostname, address)
            try:
                path = parts.path + ("?" + parts.query if parts.query else "")
                conn.request("GET", path, headers={"Host": parts.hostname, "User-Agent": "NipaPlay/1.0", "Connection": "close"})
                response = conn.getresponse()
                if response.status != 200:
                    raise ValueError(f"image source status {response.status}")
                body = response.read(12 * 1024 * 1024 + 1)
                if not body or len(body) > 12 * 1024 * 1024:
                    raise ValueError("invalid image response size")
                with self.lock:
                    self.preferred[parts.hostname] = address
                return body, response.getheader("Content-Type", "")
            except Exception as error:
                last = error
            finally:
                conn.close()
        raise OSError("image source unavailable") from last

    def poster(self, subject):
        body, _ = self.get(f"https://api.bgm.tv/v0/subjects/{subject}")
        image_url = json.loads(body)["images"]["large"]
        image, content_type = self.get(image_url)
        if not content_type.startswith("image/"):
            raise ValueError("not an image")
        return image
