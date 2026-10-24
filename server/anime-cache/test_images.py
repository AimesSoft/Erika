import json
import ssl
import unittest
from unittest.mock import MagicMock, patch
from urllib.parse import urlsplit

from images import BangumiImages, Tunnel


def response(body, content_type="application/json", status=200):
    conn = MagicMock()
    result = conn.getresponse.return_value
    result.status = status
    result.read.return_value = body if isinstance(body, bytes) else json.dumps(body).encode()
    result.getheader.return_value = content_type
    return conn


class ImageTests(unittest.TestCase):
    def setUp(self):
        self.client = BangumiImages("http://127.0.0.1:18083")

    def test_dns_fallback_filters_private_addresses_and_reuses_cache(self):
        failed = MagicMock()
        failed.request.side_effect = OSError("offline")
        good = response({"Status": 0, "Answer": [
            {"type": 1, "TTL": 60, "data": "104.26.9.23"},
            {"type": 1, "TTL": 60, "data": "127.0.0.1"},
        ]})
        with patch("images.Tunnel", side_effect=[failed, good]) as tunnel:
            self.assertEqual(self.client.resolve("lain.bgm.tv"), ["104.26.9.23"])
            self.assertEqual(self.client.resolve("lain.bgm.tv"), ["104.26.9.23"])
            self.assertEqual(tunnel.call_count, 2)
            self.assertEqual(tunnel.call_args_list[0].args[1:3], ("dns.google", "8.8.8.8"))
        failed.close.assert_called_once()
        good.close.assert_called_once()

    def test_failed_resolvers_do_not_cache_empty_results(self):
        with patch("images.Tunnel", side_effect=lambda *_a, **_k: response({"Status": 2})):
            with self.assertRaises(OSError):
                self.client.resolve("lain.bgm.tv")
        self.assertEqual(self.client.addresses, {})

    def test_image_tries_another_address_and_prefers_the_working_one(self):
        failed = MagicMock()
        failed.request.side_effect = ssl.SSLEOFError("closed")
        good = response(b"image", "image/jpeg")
        with patch.object(self.client, "resolve", return_value=["104.26.8.23", "104.26.9.23"]), \
                patch("images.Tunnel", side_effect=[failed, good, good]) as tunnel:
            for _ in range(2):
                self.assertEqual(self.client.get("https://lain.bgm.tv/pic/cover/l/a.jpg"), (b"image", "image/jpeg"))
            self.assertEqual([call.args[2] for call in tunnel.call_args_list],
                             ["104.26.8.23", "104.26.9.23", "104.26.9.23"])
            self.assertEqual(good.request.call_args.kwargs["headers"]["Host"], "lain.bgm.tv")
        failed.close.assert_called_once()

    def test_ip_tunnel_keeps_original_tls_name_and_certificate_checks(self):
        conn = Tunnel(urlsplit("http://127.0.0.1:18083"), "lain.bgm.tv", "104.26.9.23")
        self.assertEqual(conn._tunnel_host, "104.26.9.23")
        self.assertTrue(conn._context.check_hostname)
        self.assertEqual(conn._context.verify_mode, ssl.CERT_REQUIRED)
        raw_socket = MagicMock()
        with patch("http.client.HTTPConnection.connect", side_effect=lambda c: setattr(c, "sock", raw_socket), autospec=True), \
                patch.object(conn._context, "wrap_socket") as wrap:
            conn.connect()
            wrap.assert_called_once_with(raw_socket, server_hostname="lain.bgm.tv")
        conn.close()

    def test_unexpected_image_hosts_and_non_images_are_rejected(self):
        with patch("images.Tunnel") as tunnel:
            for url in ("http://lain.bgm.tv/a.jpg", "https://localhost/a.jpg", "https://example.com/a.jpg"):
                with self.assertRaises(ValueError):
                    self.client.get(url)
            tunnel.assert_not_called()
        with patch.object(self.client, "get", side_effect=[
            (b'{"images":{"large":"https://lain.bgm.tv/a.jpg"}}', "application/json"),
            (b"error", "text/html"),
        ]):
            with self.assertRaises(ValueError):
                self.client.poster("1")


if __name__ == "__main__":
    unittest.main()
