"""Reference screenshots of the Tessera dashboard in every look, with a
mocked API (no backend needed). Run the CSCM frontend (pnpm dev), then:

    python screenshot.py <out-dir> [frontend-url]

Use the same script against the launcher's web views to compare them
with the dashboard (same mock data, same 1440x900 viewport)."""
import asyncio, json, re, sys
from playwright.async_api import async_playwright

BASE = sys.argv[2] if len(sys.argv) > 2 else "http://localhost:3000"
OUT = sys.argv[1]
USER = {"id": 1, "username": "ramon", "is_admin": False}

def srv(i, name, typ, ver, port, status, addr):
    return {"id": i, "name": name, "slug": name.lower().replace(" ", "-"), "type": typ,
            "version": ver, "port": port, "mem_min": 1024, "mem_max": 4096, "status": "created",
            "runtime_status": status, "created_at": "2026-10-01T10:00:00", "local_only": False,
            "tunnels": [{"address": addr, "local_port": port, "external_port": 46801}] if addr else [],
            "dns_records": [], "cloudflare_available": False, "playit_configured": True,
            "tunnel_error": None, "dns_error": None, "owner": {"id": 1, "username": "ramon"},
            "icon_version": None, "progress": None}

SIGNED_IN = True
SERVERS = [
    srv(1, "Survival", "paper", "1.21.4", 25565, "running", "domination-plants.tun.ply.gg"),
    srv(2, "Create Modpack", "fabric", "1.20.1", 25566, "stopped", "quiet-river.tun.ply.gg"),
    srv(3, "Bedrock Friends", "bedrock", "LATEST", 19132, "running", "sunny-hill.tun.ply.gg"),
]
STATS = {
    1: {"running": True, "state": "running", "status": "running", "health": "healthy", "cpu_percent": 23.4,
        "memory_usage_bytes": 2.1e9, "memory_limit_bytes": 4.29e9,
        "players_raw": "There are 3 of a max of 20 players online: Steve, Alex, Notch"},
    2: {"running": False, "state": "stopped", "status": "exited", "health": "none", "cpu_percent": 0,
        "memory_usage_bytes": 0, "memory_limit_bytes": 0},
    3: {"running": True, "state": "running", "status": "running", "health": "healthy", "cpu_percent": 8,
        "memory_usage_bytes": 0.9e9, "memory_limit_bytes": 2.1e9, "players_online": ["Kai"], "player_count": 1},
}
LOGS = "\n".join([
    "[10:02:11] [Server thread/INFO]: Starting minecraft server version 1.21.4",
    "[10:02:12] [Server thread/INFO]: Loading properties",
    "[10:02:14] [Server thread/WARN]: Can't keep up! Is the server overloaded?",
    "[10:02:18] [Server thread/INFO]: Done (6.912s)! For help, type \"help\"",
    "[10:05:40] [Server thread/INFO]: Steve joined the game",
    "[10:06:02] [Server thread/INFO]: Alex joined the game",
])

def body_for(path):
    p = path.split("?")[0]
    if p == "/api/auth/status": return {"authenticated": SIGNED_IN, "setup_required": False, "user": USER}
    if p == "/api/players/lists": return {"whitelist": ["Steve", "Alex"], "ops": ["Steve"], "bans": [{"name": "Griefer42", "reason": "Griefing", "created_at": "2026-10-20T12:00:00"}]}
    if p == "/api/setup": return {"completed": True, "public_available": True}
    if p == "/api/servers": return {"servers": SERVERS}
    m = re.match(r"/api/servers/(\d+)(/.*)?$", p)
    if m:
        sid, rest = int(m.group(1)), m.group(2) or ""
        if rest == "": return {"server": SERVERS[sid - 1]}
        if rest == "/stats": return STATS[sid]
        if rest == "/progress": return {}
        if rest.startswith("/logs"): return {"data": LOGS.split("\n")}
        if rest == "/players": return {"online": ["Steve", "Alex", "Notch"], "whitelist": [], "ops": ["Steve"]}
        return {}
    if p in ("/api/version", "/api/system/version"): return {"version": "1.9.3", "update_available": False}
    return {}

async def shoot(pw, design, scheme, name, path, action=None):
    browser = await pw.chromium.launch()
    ctx = await browser.new_context(viewport={"width": 1440, "height": 900}, device_scale_factor=1,
                                    color_scheme=scheme, reduced_motion="reduce")
    await ctx.add_init_script(f"localStorage.setItem('cscm_jwt','x');localStorage.setItem('cscm-design','{design}')")
    async def handle(route):
        url = route.request.url
        path = url[len(BASE):]
        await route.fulfill(status=200, content_type="application/json", body=json.dumps(body_for(path)))
    await ctx.route(re.compile(r".*/api/.*"), handle)
    page = await ctx.new_page()
    await page.goto(BASE + path, wait_until="networkidle")
    await page.add_style_tag(content='nextjs-portal{display:none!important}')
    await page.wait_for_timeout(1500)
    if action: await action(page)
    await page.screenshot(path=f"{OUT}/{name}.png")
    await browser.close()

async def main():
    async with async_playwright() as pw:
        global SIGNED_IN
        for design, scheme, tag in [("modern", "dark", "modern-dark"), ("modern", "light", "modern-light"), ("classic", "dark", "classic")]:
            await shoot(pw, design, scheme, f"dashboard-{tag}", "/servers")
            await shoot(pw, design, scheme, f"server-{tag}", "/servers/1")
            await shoot(pw, design, scheme, f"settings-{tag}", "/settings")
            SIGNED_IN = False
            await shoot(pw, design, scheme, f"signin-{tag}", "/servers")
            SIGNED_IN = True

asyncio.run(main())
