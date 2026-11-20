#!/usr/bin/env python3
"""Print a Home Assistant access token, for feeding the Rust binary.

Usage: python3 get_token.py [base_url] [username] [password]
"""

import json
import sys
import urllib.parse
import urllib.request

base = sys.argv[1] if len(sys.argv) > 1 else "http://127.0.0.1:12000"
username = sys.argv[2] if len(sys.argv) > 2 else "admin"
password = sys.argv[3] if len(sys.argv) > 3 else "homeassistant"
client = base + "/"


def post(path, payload, form=False):
    if form:
        data = urllib.parse.urlencode(payload).encode()
        ctype = "application/x-www-form-urlencoded"
    else:
        data = json.dumps(payload).encode()
        ctype = "application/json"
    req = urllib.request.Request(
        base + path, data=data, headers={"Content-Type": ctype}, method="POST"
    )
    with urllib.request.urlopen(req) as resp:
        return json.loads(resp.read().decode())


flow = post(
    "/auth/login_flow",
    {"client_id": client, "handler": ["homeassistant", None], "redirect_uri": client},
)
step = post(
    f"/auth/login_flow/{flow['flow_id']}",
    {"client_id": client, "username": username, "password": password},
)
token = post(
    "/auth/token",
    {"client_id": client, "grant_type": "authorization_code", "code": step["result"]},
    form=True,
)
print(token["access_token"])
