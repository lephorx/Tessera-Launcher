import io, json, os, re, sys, time, zipfile, requests
env = dict(l.split("=",1) for l in open(".env.local").read().splitlines() if "=" in l and not l.startswith("#"))
AUTH = env["NEON_AUTH_BASE_URL"].strip('"'); API = env["NEON_FUNCTION_API_BASE_URL"].strip('"').rstrip("/") + "/v1"
O = {"origin": "http://localhost:1420"}
import subprocess
DBURL = env["DATABASE_URL_UNPOOLED"].strip('"')
psql = lambda sql: subprocess.run(["psql", DBURL, "-qtA", "-c", sql], check=True, capture_output=True, text=True).stdout.strip()
def account(tag, verified=True):
    # sign-in needs a verified email; example.com never gets the code, so the test verifies it itself
    s = requests.Session(); email = f"e2e{tag}{int(time.time())}@example.com"
    r = s.post(f"{AUTH}/sign-up/email", json={"email": email, "password": "correct horse battery", "name": tag}, headers=O); assert r.ok, r.text
    psql(f"""update neon_auth."user" set "emailVerified" = true where email = '{email}'""")
    r = s.post(f"{AUTH}/sign-in/email", json={"email": email, "password": "correct horse battery"}, headers=O); assert r.ok, r.text
    if not verified: psql(f"""update neon_auth."user" set "emailVerified" = false where email = '{email}'""")
    jwt = s.get(f"{AUTH}/token", headers=O).json()["token"]
    return s, {"authorization": f"Bearer {jwt}"}, email
sa, A, ea = account("alice"); sb, B, eb = account("bob")
check = lambda label, ok: print(("PASS " if ok else "FAIL ") + label) or (ok or sys.exit(1))
check("unauthenticated is 401", requests.get(f"{API}/me").status_code == 401)
check("forged token is 401", requests.get(f"{API}/me", headers={"authorization": "Bearer abc.def.ghi"}).status_code == 401)
ua, ub = f"alice_{int(time.time())%100000}", f"bob_{int(time.time())%100000}"
# closed beta: seed two invites in the database (as if an admin made them), then join
ida0 = requests.get(f"{API}/me", headers=A).json()["user"]["id"]
check("no invite: locked out", requests.get(f"{API}/shares/mine", headers=A).status_code == 403)
check("no invite: /me works", requests.get(f"{API}/me", headers=A).json()["user"]["member"] is False)
seed = [f"e2eA{int(time.time())%100000}", f"e2eB{int(time.time())%100000}"]
subprocess.run(["psql", DBURL, "-q", "-c", f"insert into invites (code, created_by) values ('{seed[0]}', '{ida0}'), ('{seed[1]}', '{ida0}')"], check=True)
check("public invite check", requests.get(f"{API}/invites/{seed[0]}").json()["valid"] is True)
check("bad username refused", requests.post(f"{API}/invites/{seed[0]}/redeem", json={"username": "a b"}, headers=A).status_code == 400)
check("A joins with invite", requests.post(f"{API}/invites/{seed[0]}/redeem", json={"username": ua}, headers=A).status_code == 201)
check("invite works only once", requests.post(f"{API}/invites/{seed[0]}/redeem", json={"username": ub}, headers=B).status_code == 410)
check("used invite shows as used", requests.get(f"{API}/invites/{seed[0]}").json()["valid"] is False)
check("username taken -> 409", requests.post(f"{API}/invites/{seed[1]}/redeem", json={"username": ua}, headers=B).status_code == 409)
check("B joins", requests.post(f"{API}/invites/{seed[1]}/redeem", json={"username": ub}, headers=B).status_code == 201)
check("can't join twice", requests.post(f"{API}/invites/{seed[1]}/redeem", json={"username": ub}, headers=B).status_code == 409)
check("rename still works", requests.put(f"{API}/me/username", json={"username": ub}, headers=B).ok)
inv = requests.get(f"{API}/invites", headers=A).json()
check("members may make 3 (2 used)", inv["limit"] == 3 and inv["remaining"] == 1)
c3 = requests.post(f"{API}/invites", headers=A).json()["invite"]["code"]
check("limit reached", requests.post(f"{API}/invites", headers=A).status_code == 409)
check("B can't revoke A's invite", requests.delete(f"{API}/invites/{c3}", headers=B).status_code == 404)
check("revoking frees a slot", requests.delete(f"{API}/invites/{c3}", headers=A).ok and requests.post(f"{API}/invites", headers=A).status_code == 201)
check("used invites can't be revoked", requests.delete(f"{API}/invites/{seed[0]}", headers=A).status_code == 404)
buf = io.BytesIO(); z = zipfile.ZipFile(buf, "w"); z.writestr("tessera.pack.json", json.dumps({"format":1,"name":"E2E","version":"1","minecraft":"1.21.1","loader":{"type":"fabric","version":"0.16.9"},"content":[]})); z.close()
q = {"name": "E2E Pack", "game_version": "1.21.1", "loader": "fabric"}
check("not a zip -> 400", requests.post(f"{API}/shares", params=q, data=b"hello", headers=A).status_code == 400)
r = requests.post(f"{API}/shares", params=q, data=buf.getvalue(), headers=A); check("create share", r.status_code == 201); code = r.json()["share"]["code"]
check("public lookup works", requests.get(f"{API}/shares/{code}").json()["share"]["owner"] == ua)
check("B can't delete A's share", requests.delete(f"{API}/shares/{code}", headers=B).status_code == 404)
check("B can't add members", requests.post(f"{API}/shares/{code}/members", json={"username": ub}, headers=B).status_code == 404)
# new members are private by default; B opens up for the share checks
check("new members: friend requests from anyone, packs from friends", requests.get(f"{API}/me/privacy", headers=B).json()["privacy"] == {"friend_requests": "everyone", "share_invites": "friends"})
check("A and B accept requests and shares from anyone", all(requests.put(f"{API}/me/privacy", json={"friend_requests": "everyone", "share_invites": "everyone"}, headers=h).ok for h in (A, B)))
check("sharing needs friendship", requests.post(f"{API}/shares/{code}/members", json={"username": ub}, headers=A).json().get("error") == "not_friends")
check("B can't download a pack by its link alone", requests.get(f"{API}/shares/{code}/download", headers=B).status_code == 403)
_ida = requests.get(f"{API}/me", headers=A).json()["user"]["id"]
check("A and B become friends", requests.post(f"{API}/friends", json={"username": ub}, headers=A).ok and requests.post(f"{API}/friends/{_ida}/accept", headers=B).ok)
n = requests.get(f"{API}/notifications", headers=A).json()
check("A is told B accepted", any(x["kind"] == "friend_accepted" and x["username"] == ub for x in n["notifications"]) and n["unread"] >= 1)
check("seen clears unread", requests.post(f"{API}/notifications/seen", headers=A).ok and requests.get(f"{API}/notifications", headers=A).json()["unread"] == 0)
check("add B by username", requests.post(f"{API}/shares/{code}/members", json={"username": ub}, headers=A).status_code == 201)
check("B is invited to the pack", any(x["kind"] == "share_invite" and x["code"] == code and x["username"] == ua for x in requests.get(f"{API}/notifications", headers=B).json()["notifications"]))
check("unknown username -> 404", requests.post(f"{API}/shares/{code}/members", json={"username": "nobody_x_y"}, headers=A).status_code == 404)
# an invite isn't the pack yet: B accepts first
check("invite: listed for B", [s["code"] for s in requests.get(f"{API}/shares/invites", headers=B).json()["invites"]] == [code])
check("invite: not received yet", not requests.get(f"{API}/shares/received", headers=B).json()["shares"])
check("invite: no download yet", requests.get(f"{API}/shares/{code}/download", headers=B).json().get("error") == "friends_only")
check("invite: no role yet", requests.get(f"{API}/shares/{code}", headers=B).json()["share"]["role"] is None)
check("invite: A sees B as pending", requests.get(f"{API}/shares/mine", headers=A).json()["shares"][0]["members"][0]["pending"] is True)
check("invite: A can't hand it to B yet", requests.post(f"{API}/shares/{code}/transfer", json={"user_id": requests.get(f"{API}/me", headers=B).json()["user"]["id"]}, headers=A).status_code == 404)
check("only B accepts B's invite", requests.post(f"{API}/shares/{code}/accept", headers=A).status_code == 404)
check("B accepts", requests.post(f"{API}/shares/{code}/accept", headers=B).json() == {"ok": True, "role": "viewer"})
check("accepting twice is fine", requests.post(f"{API}/shares/{code}/accept", headers=B).ok)
check("accepted: no invite left", not requests.get(f"{API}/shares/invites", headers=B).json()["invites"] and not any(x["kind"] == "share_invite" for x in requests.get(f"{API}/notifications", headers=B).json()["notifications"]))
check("B sees it under received", any(s["code"] == code for s in requests.get(f"{API}/shares/received", headers=B).json()["shares"]))
mine = requests.get(f"{API}/shares/mine", headers=A).json()["shares"]; check("A sees member B", mine[0]["members"][0]["username"] == ub and mine[0]["members"][0]["pending"] is False)
dl = requests.get(f"{API}/shares/{code}/download", headers=B); check("B gets a download URL", dl.ok)
check("download returns the pack", requests.get(dl.json()["url"]).content == buf.getvalue())
check("set expiry never", requests.patch(f"{API}/shares/{code}", json={"expires_in_days": None}, headers=A).json()["share"]["expires_at"] is None)
check("bad expiry -> 400", requests.patch(f"{API}/shares/{code}", json={"expires_in_days": 5}, headers=A).status_code == 400)
check("A deletes share", requests.delete(f"{API}/shares/{code}", headers=A).ok)
check("deleted share is gone", requests.get(f"{API}/shares/{code}").status_code == 404)
# roles and owner transfer
def pack(mc="1.21.1", loader="fabric", lv="0.16.9", extra=""):
    b = io.BytesIO(); z = zipfile.ZipFile(b, "w")
    z.writestr("tessera.pack.json", json.dumps({"format":1,"name":"E2E","version":"1","minecraft":mc,"loader":{"type":loader,"version":lv},"content":[]}))
    if extra: z.writestr("overrides/config/x.txt", extra)
    z.close(); return b.getvalue()
r = requests.post(f"{API}/shares", params=q, data=pack(), headers=A); code2 = r.json()["share"]["code"]
idb = requests.post(f"{API}/shares/{code2}/members", json={"username": ub}, headers=A).json()["member"]["user_id"]
requests.post(f"{API}/shares/{code2}/accept", headers=B)
check("member starts as viewer", requests.get(f"{API}/shares/{code2}", headers=B).json()["share"]["role"] == "viewer")
check("viewer can't upload", requests.put(f"{API}/shares/{code2}", params=q, data=pack(extra="b"), headers=B).status_code == 403)
check("B can't change roles", requests.patch(f"{API}/shares/{code2}/members/{idb}", json={"role": "editor"}, headers=B).status_code == 404)
check("owner makes B editor", requests.patch(f"{API}/shares/{code2}/members/{idb}", json={"role": "editor"}, headers=A).ok)
check("editor uploads new content", requests.put(f"{API}/shares/{code2}", params=q, data=pack(extra="new mods"), headers=B).ok)
check("editor can't change Minecraft", requests.put(f"{API}/shares/{code2}", params=q, data=pack(mc="1.21.4"), headers=B).status_code == 403)
check("editor can't change loader version", requests.put(f"{API}/shares/{code2}", params=q, data=pack(lv="0.17.0"), headers=B).status_code == 403)
check("pack with lying query is read from the manifest", requests.put(f"{API}/shares/{code2}", params={**q, "game_version": "1.21.1"}, data=pack(mc="1.20.1"), headers=B).status_code == 403)
rev = requests.get(f"{API}/shares/sync", params={"codes": code2}, headers=A).json()["shares"][0]["revision"]
check("sync lists revision for members too", requests.get(f"{API}/shares/sync", params={"codes": code2}, headers=B).json()["shares"][0]["revision"] == rev)
check("sync needs sign-in", requests.get(f"{API}/shares/sync", params={"codes": code2}).status_code in (401, 404))
r1 = requests.put(f"{API}/shares/{code2}", params={**q, "base_revision": rev}, data=pack(extra="live 1"), headers=B)
check("upload on the current revision bumps it", r1.ok and r1.json()["revision"] == rev + 1)
check("upload from an old copy -> 409", requests.put(f"{API}/shares/{code2}", params={**q, "base_revision": rev}, data=pack(extra="stale"), headers=A).status_code == 409)
s_ = requests.get(f"{API}/shares/sync", params={"codes": code2}, headers=A).json()["shares"][0]
check("sync says who changed it", s_["updated_by"] == ub and s_["mine"] is False)
check("owner can change versions", requests.put(f"{API}/shares/{code2}", params=q, data=pack(mc="1.21.4", lv="0.17.0"), headers=A).ok)
check("shares/mine shows role", requests.get(f"{API}/shares/mine", headers=A).json()["shares"][0]["members"][0]["role"] == "editor")
check("transfer needs a member", requests.post(f"{API}/shares/{code2}/transfer", json={"user_id": "00000000-0000-0000-0000-000000000000"}, headers=A).status_code == 404)
check("B can't take the share", requests.post(f"{API}/shares/{code2}/transfer", json={"user_id": idb}, headers=B).status_code == 404)
check("A transfers to B", requests.post(f"{API}/shares/{code2}/transfer", json={"user_id": idb}, headers=A).ok)
s2 = requests.get(f"{API}/shares/{code2}", headers=B).json()["share"]
check("B owns it now", s2["role"] == "owner" and s2["owner"] == ub)
check("A stayed as editor", requests.get(f"{API}/shares/{code2}", headers=A).json()["share"]["role"] == "editor")
check("A can't delete it anymore", requests.delete(f"{API}/shares/{code2}", headers=A).status_code == 404)
ida = requests.get(f"{API}/me", headers=A).json()["user"]["id"]
check("A leaves the share", requests.delete(f"{API}/shares/{code2}/members/{ida}", headers=A).ok and requests.get(f"{API}/shares/{code2}", headers=A).json()["share"]["role"] is None)
check("new owner deletes it", requests.delete(f"{API}/shares/{code2}", headers=B).ok)
code3 = requests.post(f"{API}/shares", params=q, data=pack(), headers=A).json()["share"]["code"]
requests.post(f"{API}/shares/{code3}/members", json={"username": ub}, headers=A)
check("B declines an invite", requests.delete(f"{API}/shares/{code3}/members/{idb}", headers=B).ok and not requests.get(f"{API}/shares/invites", headers=B).json()["invites"])
requests.post(f"{API}/shares/{code3}/members", json={"username": ub}, headers=A); requests.post(f"{API}/shares/{code3}/accept", headers=B)
check("accepted after being asked again", requests.get(f"{API}/shares/{code3}", headers=B).json()["share"]["role"] == "viewer")
check("unfriending takes B off A's packs", requests.delete(f"{API}/friends/{ida}", headers=B).ok and requests.get(f"{API}/shares/{code3}", headers=B).json()["share"]["role"] is None)
requests.delete(f"{API}/shares/{code3}", headers=A)
F = lambda h: {f["username"]: f["status"] for f in requests.get(f"{API}/friends", headers=h).json()["friends"]}
check("friend request to self -> 400", requests.post(f"{API}/friends", json={"username": ua}, headers=A).status_code == 400)
check("A asks B", requests.post(f"{API}/friends", json={"username": ub}, headers=A).json()["friend"]["status"] == "outgoing")
check("A sees outgoing", F(A).get(ub) == "outgoing")
check("B sees incoming", F(B).get(ua) == "incoming")
ida = next(f["user_id"] for f in requests.get(f"{API}/friends", headers=B).json()["friends"])
check("bad user id -> 404", requests.post(f"{API}/friends/not-a-uuid/accept", headers=B).status_code == 404)
check("A can't accept own request", requests.post(f"{API}/friends/{ida}/accept", headers=A).status_code == 404)
check("B accepts", requests.post(f"{API}/friends/{ida}/accept", headers=B).ok)
check("now friends both ways", F(A).get(ub) == "friend" and F(B).get(ua) == "friend")
check("B unfriends", requests.delete(f"{API}/friends/{ida}", headers=B).ok and F(A) == {})
check("B asks, A asking back accepts", requests.post(f"{API}/friends", json={"username": ua}, headers=B).ok
      and requests.post(f"{API}/friends", json={"username": ub}, headers=A).json()["friend"]["status"] == "friend")
idb_ = requests.get(f"{API}/me", headers=B).json()["user"]["id"]
check("privacy is as B set it", requests.get(f"{API}/me/privacy", headers=B).json()["privacy"] == {"friend_requests": "everyone", "share_invites": "everyone"})
check("bad privacy value -> 400", requests.put(f"{API}/me/privacy", json={"friend_requests": "friends"}, headers=B).status_code == 400)
# what launchers up to 0.6.12 send: the unchanged setting as null
r = requests.put(f"{API}/me/privacy", json={"friend_requests": "everyone", "share_invites": None}, headers=B)
check("privacy with a null setting -> keeps it", r.ok and r.json()["privacy"] == {"friend_requests": "everyone", "share_invites": "everyone"})
check("B: shares from friends only", requests.put(f"{API}/me/privacy", json={"share_invites": "friends"}, headers=B).json()["privacy"]["share_invites"] == "friends")
code3 = requests.post(f"{API}/shares", params=q, data=pack(), headers=A).json()["share"]["code"]
check("friend A may still share with B", requests.post(f"{API}/shares/{code3}/members", json={"username": ub}, headers=A).status_code == 201)
check("B blocks A", requests.post(f"{API}/blocks", json={"username": ua}, headers=B).status_code == 201)
check("blocking unfriends", F(A) == {} and F(B) == {})
check("blocking takes B off A's share", requests.get(f"{API}/shares/{code3}", headers=B).json()["share"]["role"] is None)
check("blocked A can't ask B", requests.post(f"{API}/friends", json={"username": ub}, headers=A).status_code == 403)
check("blocked A can't share with B", requests.post(f"{API}/shares/{code3}/members", json={"username": ub}, headers=A).status_code == 403)
check("B lists the block", [b["username"] for b in requests.get(f"{API}/blocks", headers=B).json()["blocks"]] == [ua])
check("B unblocks A", requests.delete(f"{API}/blocks/{ida}", headers=B).ok and requests.get(f"{API}/blocks", headers=B).json()["blocks"] == [])
check("not friends now: B takes no shares from A", requests.post(f"{API}/shares/{code3}/members", json={"username": ub}, headers=A).status_code == 403)
check("B: no friend requests", requests.put(f"{API}/me/privacy", json={"friend_requests": "no_one"}, headers=B).ok)
check("A can't ask B", requests.post(f"{API}/friends", json={"username": ub}, headers=A).status_code == 403)
check("B: friends of friends", requests.put(f"{API}/me/privacy", json={"friend_requests": "friends_of_friends"}, headers=B).ok)
check("no mutual friend: refused", requests.post(f"{API}/friends", json={"username": ub}, headers=A).status_code == 403)
check("B can still ask A", requests.post(f"{API}/friends", json={"username": ua}, headers=B).json()["friend"]["status"] == "outgoing")
check("A accepting by asking back still works", requests.post(f"{API}/friends", json={"username": ub}, headers=A).json()["friend"]["status"] == "friend")
requests.delete(f"{API}/shares/{code3}", headers=A)
# email settings
check("email notices on by default", requests.get(f"{API}/me/email", headers=A).json()["email"]["friends"] is True)
check("turn friend emails off", requests.put(f"{API}/me/email", json={"friends": False}, headers=A).json()["email"] == {"friends": False, "shares": True})
check("bad unsubscribe link -> 400", requests.post(f"{API}/email/unsubscribe", params={"u": ida, "k": "friends", "t": "nope"}).status_code == 400)
# two-step sign-in: codes go by email, so the test puts them in the database itself (only hashes are kept)
import hashlib
def seed_code(uid, purpose, code="123456"):
    h = hashlib.sha256(f"{uid}:{purpose}:{code}".encode()).hexdigest()
    subprocess.run(["psql", DBURL, "-q", "-c", f"insert into email_codes (user_id, purpose, code_hash, expires_at) values ('{uid}', '{purpose}', '{h}', now() + interval '10 minutes') on conflict (user_id, purpose) do update set code_hash = excluded.code_hash, attempts = 0, expires_at = excluded.expires_at"], check=True)
check("2fa off: no code to disable", requests.post(f"{API}/me/2fa/code", json={"purpose": "disable"}, headers=A).status_code in (409, 503))
check("enable without a code -> expired", requests.post(f"{API}/me/2fa/enable", json={"code": "123456"}, headers=A).json()["error"] == "code_expired")
seed_code(ida, "enable")
check("wrong code -> 400", requests.post(f"{API}/me/2fa/enable", json={"code": "000000"}, headers=A).json()["error"] == "wrong_code")
r = requests.post(f"{API}/me/2fa/enable", json={"code": "123456", "label": "e2e"}, headers=A)
check("enable with the code", r.ok and len(r.json()["device_token"]) > 20); devA = r.json()["device_token"]
me_ = requests.get(f"{API}/me", headers=A).json()["user"]
check("new device: /me says code needed, shows little", me_["two_factor_required"] is True and "is_admin" not in me_)
check("new device: everything else locked", requests.get(f"{API}/friends", headers=A).json().get("error") == "two_factor_required")
check("new device: can't turn 2fa off", requests.post(f"{API}/me/2fa/code", json={"purpose": "disable"}, headers=A).status_code == 403)
AD = {**A, "x-tessera-device": devA}
check("code works once", requests.post(f"{API}/me/2fa/enable", json={"code": "123456"}, headers=AD).json()["error"] == "code_expired")
check("trusted device gets in", requests.get(f"{API}/friends", headers=AD).ok and requests.get(f"{API}/me", headers=AD).json()["user"]["two_factor"] is True)
check("forged device token refused", requests.get(f"{API}/friends", headers={**A, "x-tessera-device": "x" * 43}).status_code == 403)
seed_code(ida, "sign_in")
for _ in range(5): requests.post(f"{API}/me/2fa/verify", json={"code": "999999"}, headers=A)
check("five wrong codes lock it", requests.post(f"{API}/me/2fa/verify", json={"code": "123456"}, headers=A).status_code == 429)
seed_code(ida, "sign_in")
r = requests.post(f"{API}/me/2fa/verify", json={"code": "123456", "label": "second"}, headers=A)
check("sign-in code trusts a second device", r.ok); AD2 = {**A, "x-tessera-device": r.json()["device_token"]}
check("both devices listed", len(requests.get(f"{API}/me/2fa/devices", headers=AD2).json()["devices"]) == 2)
check("forget the others", requests.delete(f"{API}/me/2fa/devices", headers=AD2).json()["removed"] == 1 and requests.get(f"{API}/friends", headers=AD).status_code == 403)
seed_code(ida, "disable")
check("turn 2fa off", requests.post(f"{API}/me/2fa/disable", json={"code": "123456"}, headers=AD2).ok)
check("off: no device needed", requests.get(f"{API}/friends", headers=A).ok and requests.get(f"{API}/me", headers=A).json()["user"]["two_factor_required"] is False)
check("news is public", len(requests.get(f"{API}/news").json()["news"]) > 0)
# verified email: "you're verified" goes out once per address (example.com gets none, the log still counts it)
check("verified mail logged once", psql(f"select count(*) from email_log where user_id = '{ida}' and kind = 'email_verified'") == "1")
requests.get(f"{API}/me", headers=A)
check("not twice", psql(f"select count(*) from email_log where user_id = '{ida}' and kind = 'email_verified'") == "1")
# an unverified member only gets the open calls
psql(f"""update neon_auth."user" set "emailVerified" = false where id = '{ida}'""")
check("unverified: friends locked", requests.get(f"{API}/friends", headers=A).json().get("error") == "email_unverified")
check("unverified: /me still works", requests.get(f"{API}/me", headers=A).json()["user"]["email_verified"] is False)
# fixing the address of an unverified account: password-checked, public
ch = lambda **b: requests.post(f"{API}/email/change", json=b)
check("change: wrong password -> 401", ch(email=ea, password="nope nope", new_email="x@example.com").status_code == 401)
check("change: unknown account -> 401", ch(email="nobody-e2e@example.com", password="correct horse battery", new_email="x@example.com").status_code == 401)
check("change: taken address -> 409", ch(email=ea, password="correct horse battery", new_email=eb.upper()).json().get("error") == "email_taken")
ea2 = ea.replace("@", "-new@")
r = ch(email=ea, password="correct horse battery", new_email=ea2)
check("change: works", r.ok and r.json()["email"] == ea2 and requests.get(f"{API}/me", headers=A).json()["user"]["email"] == ea2)
check("change: old address can't sign in", requests.post(f"{AUTH}/sign-in/email", json={"email": ea, "password": "correct horse battery"}, headers=O).status_code == 401)
check("change: new one asks for the code", requests.post(f"{AUTH}/sign-in/email", json={"email": ea2, "password": "correct horse battery"}, headers=O).json().get("code") == "EMAIL_NOT_VERIFIED")
psql(f"""update neon_auth."user" set "emailVerified" = true where id = '{ida}'""")
check("change: verified address stays", ch(email=ea2, password="correct horse battery", new_email=ea).json().get("error") == "already_verified")
check("verified again: friends open", requests.get(f"{API}/friends", headers=A).ok)
ea = ea2
# names: no slurs, swearing or notorious people, nothing pretending to be staff, no odd symbols
for bad in ["diddy_fan", "3pst3in", "xX_fuck_Xx", "BigAss", "admin", "TesseraTeam", "le__x", "_lead", "1234", "le-x"]:
    check(f"username {bad} refused", requests.put(f"{API}/me/username", json={"username": bad}, headers=B).json().get("error") == "invalid_username")
check("a fine username still works", requests.put(f"{API}/me/username", json={"username": ub}, headers=B).ok)
check("pack name with Epstein refused", requests.post(f"{API}/shares", params={**q, "name": "Epstein Island"}, data=pack(), headers=A).json().get("error") == "invalid_name")
# signed-in devices: the caller says which session is its own (sha256 of the token)
import hashlib, urllib.parse
def shash(sess): return hashlib.sha256(urllib.parse.unquote(next(c.value for c in sess.cookies if c.name.endswith("session_token"))).split(".")[0].encode()).hexdigest()
A2s = requests.Session(); A2s.post(f"{AUTH}/sign-in/email", json={"email": ea, "password": "correct horse battery"}, headers={**O, "user-agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/130.0 Safari/537.36"})
AS = {**A, "x-tessera-session": shash(sa)}
ss = requests.get(f"{API}/me/sessions", headers=AS).json()["sessions"]
check("sessions: both listed, this one marked", len(ss) >= 2 and sum(x["current"] for x in ss) == 1)
check("sessions: browser named", any(x["device"] == "Chrome on Windows (tesseralauncher.com)" for x in ss))
check("sign out everywhere else needs to know which is mine", requests.delete(f"{API}/me/sessions", headers=A).status_code == 400)
check("sign out everywhere else", requests.delete(f"{API}/me/sessions", headers=AS).json()["removed"] >= 1)
check("the other one is signed out", A2s.get(f"{AUTH}/token", headers=O).status_code == 401)
check("this one still works", requests.get(f"{API}/me/sessions", headers=AS).json()["sessions"][0]["current"] is True)
check("B can't sign out A's session", requests.delete(f"{API}/me/sessions/{ss[0]['id']}", headers=B).status_code == 404)
# deleting the account takes the password and the username typed out
check("delete: nothing sent -> 400", requests.delete(f"{API}/me", headers=B).json().get("error") == "confirm_required")
check("delete: wrong username -> 400", requests.delete(f"{API}/me", json={"password": "correct horse battery", "confirm": "someone"}, headers=B).json().get("error") == "wrong_confirm")
check("delete: wrong password -> 401", requests.delete(f"{API}/me", json={"password": "nope nope", "confirm": ub}, headers=B).json().get("error") == "wrong_password")
check("delete: both right", requests.delete(f"{API}/me", json={"password": "correct horse battery", "confirm": ub.upper()}, headers=B).status_code in (200, 204))
check("deleted: B is gone", psql(f"""select count(*) from neon_auth."user" where email = '{eb}'""") == "0")
subprocess.run(["psql", DBURL, "-q", "-c", f"delete from invites where created_by = '{ida0}'"], check=True)
open("/tmp/neon_e2e_users", "w").write(f"{ea}\n{eb}\n")
