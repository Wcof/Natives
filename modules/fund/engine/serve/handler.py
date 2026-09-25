"""HTTP Handler：API路由 + 静态看板文件服务。"""
from __future__ import annotations

import json
import logging
import os
import time
from http.server import BaseHTTPRequestHandler
from urllib.parse import parse_qs, urlparse

log = logging.getLogger("trend_app")


# ---- HTTP Handler ----
class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        pass  # 静默日志，避免刷屏

    def _send_no_cache_headers(self):
        """防止浏览器缓存旧版页面"""
        self.send_header("Cache-Control", "no-cache, no-store, must-revalidate")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")

    def _json(self, data: dict, status: int = 200):
        body = json.dumps(data, ensure_ascii=False, default=str).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Access-Control-Allow-Origin", "*")
        self._send_no_cache_headers()
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _html(self, content: bytes, content_type: str = "text/html"):
        self.send_response(200)
        self.send_header("Content-Type", f"{content_type}; charset=utf-8")
        self._send_no_cache_headers()
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def do_GET(self):
        parsed = urlparse(self.path)
        path = parsed.path
        params = parse_qs(parsed.query)

        # API路由
        if path.startswith("/api/"):
            try:
                # 延迟导入避免与app模块的循环导入（handler函数仍在app层）
                import app

                if path == "/api/health":
                    self._json({"status": "ok", "time": time.strftime("%H:%M:%S")})
                elif path == "/api/analyze":
                    self._json(app.handle_analyze(params))
                elif path == "/api/quote":
                    self._json(app.handle_quote(params))
                elif path == "/api/search":
                    self._json(app.handle_search(params))
                elif path == "/api/kline":
                    self._json(app.handle_kline(params))
                elif path == "/api/minute":
                    self._json(app.handle_minute(params))
                elif path == "/api/chanlun_minute":
                    self._json(app.handle_chanlun_minute(params))
                elif path == "/api/chanlun_daily":
                    self._json(app.handle_chanlun_daily(params))
                elif path == "/api/realtime_flow":
                    self._json(app.handle_realtime_flow(params))
                elif path == "/api/scan":
                    self._json(app.handle_scan(params))
                else:
                    self._json({"error": "未知API"}, 404)
            except Exception as e:
                log.error(f"API错误: {e}", exc_info=True)
                self._json({"error": str(e)}, 500)
            return

        # 静态文件（看板）
        import app

        if path == "/" or path == "/index.html":
            filepath = os.path.join(app.DASHBOARD_DIR, "index.html")
        else:
            # 安全处理静态文件路径
            safe_path = path.lstrip("/")
            filepath = os.path.normpath(os.path.join(app.DASHBOARD_DIR, safe_path))
            if not filepath.startswith(app.DASHBOARD_DIR):
                self._json({"error": "禁止访问"}, 403)
                return

        if os.path.isfile(filepath):
            ext = os.path.splitext(filepath)[1].lower()
            ct = {
                ".html": "text/html", ".js": "application/javascript",
                ".css": "text/css", ".png": "image/png", ".jpg": "image/jpeg",
                ".svg": "image/svg+xml", ".ico": "image/x-icon",
            }.get(ext, "application/octet-stream")
            with open(filepath, "rb") as f:
                self._html(f.read(), ct)
        else:
            self._json({"error": "文件不存在"}, 404)

    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.end_headers()
