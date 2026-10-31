import io, json, os, re, sys, time, zipfile, requests
env = dict(l.split("=",1) for l in open(".env.local").read().splitlines() if "=" in l and not l.startswith("#"))
AUTH = env["NEON_AUTH_BASE_URL"].strip('"'); API = env["NEON_FUNCTION_API_BASE_URL"].strip('"').rstrip("/") + "/v1"
O = {"origin": "http://localhost:1420"}
def account(tag):
    s = requests.Session(); email = f"e2e{tag}{int(time.time())}@example.com"
    r = s.post(f"{AUTH}/sign-up/email", json={"email": email, "password": "correct horse battery", "name": tag}, headers=O); assert r.ok, r.text
    jwt = s.get(f"{AUTH}/token", headers=O).json()["token"]
    return s, {"authorization": f"Bearer {jwt}"}, email
sa, A, ea = account("alice"); sb, B, eb = account("bob")
check = lambda label, ok: print(("PASS " if ok else "FAIL ") + label) or (ok or sys.exit(1))
check("unauthenticated is 401", requests.get(f"{API}/me").status_code == 401)
check("forged token is 401", requests.get(f"{API}/me", headers={"authorization": "Bearer abc.def.ghi"}).status_code == 401)
ua, ub = f"alice_{int(time.time())%100000}", f"bob_{int(time.time())%100000}"
# closed beta: seed two invites in the database (as if an admin made them), then join
import subprocess
DBURL = env["DATABASE_URL_UNPOOLED"].strip('"')
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
check("B is told about the pack", any(x["kind"] == "share_added" and x["code"] == code and x["username"] == ua for x in requests.get(f"{API}/notifications", headers=B).json()["notifications"]))
check("unknown username -> 404", requests.post(f"{API}/shares/{code}/members", json={"username": "nobody_x_y"}, headers=A).status_code == 404)
check("B sees it under received", any(s["code"] == code for s in requests.get(f"{API}/shares/received", headers=B).json()["shares"]))
mine = requests.get(f"{API}/shares/mine", headers=A).json()["shares"]; check("A sees member B", mine[0]["members"][0]["username"] == ub)
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
check("news is public", len(requests.get(f"{API}/news").json()["news"]) > 0)
subprocess.run(["psql", DBURL, "-q", "-c", f"delete from invites where created_by = '{ida0}'"], check=True)
open("/tmp/neon_e2e_users", "w").write(f"{ea}\n{eb}\n")
