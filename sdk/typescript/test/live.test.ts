// Contre le VRAI serveur CSTL (target/debug/cstl_parser). Saute s'il n'est pas compile.
import { test, before, after } from "node:test";
import assert from "node:assert/strict";
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { connect } from "node:net";
import { CstlAgent, CstlGuard, ActionDenied, ApprovalRequired, Unauthorized, PolicyNotConfirmed, ControlDenied, sendRaw, render, toResponse, verifyReceipt } from "../src/index.js";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const binary = join(repo, "target", "debug", "cstl_parser");
const PORT = 6262;
const skip = !existsSync(binary) && "cargo build --bin cstl_parser requis";
let proc: ChildProcess;
let work: string;
let serverKey: string;

const open = () => new Promise<boolean>((res) => {
  const s = connect({ port: PORT, host: "127.0.0.1" }, () => { s.end(); res(true); });
  s.on("error", () => res(false));
});

before(async () => {
  if (skip) return;
  work = mkdtempSync(join(tmpdir(), "cstl_ts_"));
  const env: NodeJS.ProcessEnv = { ...process.env, CSTL_PORT: String(PORT), CSTL_REST_PORT: "9292", CSTL_KB_VERIFY: "off" };
  delete env.CSTL_DEV_MODE;
  env.CSTL_POLICY_AUTHORS = "owner_*,shared_*";
  env.CSTL_APPROVERS = "human_*";
  env.CSTL_APPROVAL_ACTIONS = "wipe_*";
  proc = spawn(binary, [], { cwd: work, env, stdio: "ignore" });
  for (let i = 0; i < 120 && !(await open()); i++) await new Promise((r) => setTimeout(r, 250));
  serverKey = readFileSync(join(work, "cstl_adn.db.server_key.pub"), "utf8").trim();
});

after(() => { proc?.kill(); });

const uid = () => Math.random().toString(36).slice(2, 10);
const agent = (name: string) => new CstlAgent(name, { port: PORT, keyFile: join(work, ".cstl", `agent_${name}.key`) });

test("autorisation avant d'agir, jeton verifie par le systeme qui execute", { skip }, async () => {
  const owner = agent(`owner_${uid()}`), bot = agent(`bot_${uid()}`);
  assert.ok(await owner.register());
  assert.ok(await bot.register());
  await owner.sendRelations([
    { type: "PERFORM", subject: bot.name, object: "wipe_db", modality: "MUST_NOT", reason: "production" },
    { type: "PERFORM", subject: bot.name, object: "refund_order", modality: "MUST_NOT", amount_above: "100" },
  ]);
  const guard = new CstlGuard(serverKey);

  const tok = await bot.authorize("read_logs");
  assert.equal(await guard.run(tok, "read_logs", () => "lu", { agent: bot.name }), "lu");
  await assert.rejects(guard.run(tok, "read_logs", () => "lu"), (e: Unauthorized) => e.reason === "already_used");

  await assert.rejects(bot.authorize("wipe_db"), (e: ActionDenied) => e.reason === "forbidden" && e.rule === "MUST_NOT");

  const t50 = await bot.authorize("refund_order", 50);
  await guard.run(t50, "refund_order", () => null, { agent: bot.name, amount: 50 });
  await assert.rejects(bot.authorize("refund_order", 500), ActionDenied);

  const t80 = await bot.authorize("refund_order", 80);
  await assert.rejects(guard.run({ ...t80, amount: "8000" }, "refund_order", () => null, { amount: 8000 }),
                       (e: Unauthorized) => e.reason === "bad_signature");
  await assert.rejects(guard.run(t80, "refund_order", () => null, { amount: 8000 }),
                       (e: Unauthorized) => e.reason === "wrong_amount");

  const p = await bot.policy();
  assert.equal(p.summary.purpose, "policy");
  assert.ok(p.rules.some((r) => r.action === "wipe_db" && r.decision === "forbidden"));
});

test("une autre cle ne parle pas au nom d'un agent ; un message modifie est refuse", { skip }, async () => {
  const bot = agent(`bot_${uid()}`);
  assert.ok(await bot.register());
  const thief = new CstlAgent(bot.name, { port: PORT, keyFile: join(work, "thief.key") });
  const r = await thief.requestAction("read_logs");
  assert.equal(r.allowed, false);
  assert.equal(r.response.intent.purpose, "signature_rejected");

  const m = bot.message("action_request", [], { action: "read_logs", request_id: uid() });
  m.intent.action = "wipe_db";
  const raw = await sendRaw(render(m), "127.0.0.1", PORT);
  assert.match(raw, /purpose=signature_rejected/);
});

test("meme identite en TypeScript et en Python (meme fichier de cle)", { skip }, async () => {
  const name = `shared_${uid()}`;
  const ts = agent(name);
  assert.ok(await ts.register());
  const py = spawnSync("python3", ["-c", `
import sys, builtins; sys.path.insert(0, ${JSON.stringify(join(repo, "sdk", "python"))})
builtins.print = lambda *a, **k: None
import cstl_llm_agent as m
a = m.CstlAgent(${JSON.stringify(name)}, "none", port=${PORT}); a.registered = True
ok, r = a.request_action("read_logs")
sys.stdout.write(r.purpose + " " + str(r.fields.get("decision")))
`], { env: { ...process.env, HOME: work }, encoding: "utf8" });
  assert.equal(py.stdout, "action_authorization allowed", py.stderr);
});

test("publishPolicy : recu signe verifie, message plus ancien refuse", { skip }, async () => {
  const owner = agent(`owner_${uid()}`), bot = agent(`bot_${uid()}`);
  assert.ok(await owner.register());
  assert.ok(await bot.register());
  const rc = await owner.publishPolicy([{ type: "PERFORM", subject: bot.name, object: "wipe_db", modality: "MUST_NOT" }], serverKey);
  assert.equal(rc.sender, owner.name);
  assert.equal((await bot.requestAction("wipe_db")).allowed, false);

  // Mauvaise cle de serveur : aucun recu ne se verifie.
  const other = new CstlAgent(`x_${uid()}`, { port: PORT, keyFile: join(work, "other.key") }).publicKey;
  await assert.rejects(owner.publishPolicy([{ type: "PERFORM", subject: bot.name, object: "read_logs", modality: "MAY" }], other, 1),
                       PolicyNotConfirmed);

  // Un message dont la sequence est plus ancienne que la derniere publiee : stale_policy.
  const old = owner.message("inform", [{ type: "PERFORM", subject: bot.name, object: "wipe_db", modality: "MAY" }], { policy_seq: "1" });
  const r = toResponse(await sendRaw(render(old), "127.0.0.1", PORT));
  assert.equal(r.intent.purpose, "stale_policy");
  assert.equal(verifyReceipt(r, old, serverKey), null);
  assert.equal((await bot.requestAction("wipe_db")).allowed, false);
});

test("revocation d'une cle volee et arret d'urgence", { skip }, async () => {
  const owner = agent(`owner_${uid()}`), bot = agent(`bot_${uid()}`), other = agent(`bot_${uid()}`);
  assert.ok(await owner.register());
  assert.ok(await bot.register());
  assert.ok(await other.register());

  await owner.emergencyStop("action:refund_*", true, "fraude");
  assert.equal((await bot.requestAction("refund_order", 5)).response.intent.reason, "emergency_stop");
  assert.equal((await bot.requestAction("read_logs")).allowed, true);
  await assert.rejects(bot.emergencyStop("action:refund_*", false), (e: ControlDenied) => e.reason === "not_authorized");
  await owner.emergencyStop("action:refund_*", false);
  assert.equal((await bot.requestAction("refund_order", 5)).allowed, true);

  await assert.rejects(bot.revokeKey({ agent: other.name }), (e: ControlDenied) => e.reason === "not_authorized");
  const r = await owner.revokeKey({ agent: bot.name, reason: "volee" });
  assert.equal(r.public_key, bot.publicKey);
  const after = await bot.requestAction("read_logs");
  assert.equal(after.allowed, false);
  assert.equal(after.response.intent.reason, "key_revoked");
  assert.equal((await other.requestAction("read_logs")).allowed, true);
});

test("accord humain, puis jeton verifie en direct (revoque avant expiration)", { skip }, async () => {
  const owner = agent(`owner_${uid()}`), bot = agent(`bot_${uid()}`), human = agent(`human_${uid()}`);
  for (const a of [owner, bot, human]) assert.ok(await a.register());
  let id = "";
  await assert.rejects(bot.authorize("wipe_cache"), (e: ApprovalRequired) => { id = e.approvalId ?? ""; return e.reason === "approval_required"; });
  assert.ok((await human.pendingApprovals()).some((r) => r.approval_id === id));
  await human.approve(id, "ok");
  const tok = await bot.authorize("wipe_cache", undefined, { approvalId: id });
  const live = new CstlGuard(serverKey, { online: { host: "127.0.0.1", port: PORT } });
  assert.equal(await live.liveStatus(tok), "valid");
  await owner.revokeKey({ agent: bot.name });
  await assert.rejects(live.run(tok, "wipe_cache", () => 1), (e: Unauthorized) => e.reason === "revoked");
  const dead = new CstlGuard(serverKey, { online: { host: "127.0.0.1", port: 1 } });
  assert.equal(await dead.liveStatus(tok), "status_unavailable");
});
