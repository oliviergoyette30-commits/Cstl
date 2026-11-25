// Le SDK TypeScript donne exactement conformance/vectors.json (comme Python et Rust).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { join, dirname } from "node:path";
import { signingBytes, signMessage, privateKeyFromRaw, publicHex, CstlGuard, Unauthorized, tokenMessage,
         render, parse, first, WireError, type AuthorizationToken } from "../src/index.js";

const repo = join(dirname(fileURLToPath(import.meta.url)), "..", "..", "..", "..");
const V = JSON.parse(readFileSync(join(repo, "conformance", "vectors.json"), "utf8"));

test("octets canoniques et signatures = vecteurs", () => {
  const key = privateKeyFromRaw(Buffer.from(V.agent_private_key, "hex"));
  assert.equal(publicHex(key), V.agent_public_key);
  for (const c of V.signing) {
    assert.equal(signingBytes(c.message).toString("utf8"), c.canonical, c.name);
    assert.equal(signMessage(c.message, key), c.message.intent.signature, c.name);
  }
});

test("message et verdicts des jetons = vecteurs", async () => {
  const t = V.token;
  const base: AuthorizationToken = { ...t.fields, signature: t.signature };
  assert.equal(tokenMessage(base).toString("utf8"), t.message);
  for (const c of t.verdicts) {
    const guard = new CstlGuard(t.server_public_key, { skew: t.skew, clock: () => c.now });
    let got = "ok";
    try {
      guard.check({ ...base, ...(c.token ?? {}) }, c.action, c.agent, c.amount);
    } catch (e) {
      got = (e as Unauthorized).reason;
    }
    assert.equal(got, c.expect, c.case);
  }
});

test("format texte : valeurs interdites refusees, guillemets si besoin, relecture", () => {
  const text = render({ version: "v5.0.0", mode: "A", meta: { encoder: "a" , produced_by: "a"},
                        intent: { purpose: "inform", sender: "a", receiver: "server", reason: "prod, jamais [x]" } });
  assert.match(text, /reason="prod, jamais \[x\]"/);
  assert.equal(first(parse(text), "INTENT_PAYLOAD").reason, "prod, jamais [x]");
  for (const bad of ['a"b', "a\nb", " a", "x---END---"]) {
    assert.throws(() => render({ version: "v5.0.0", mode: "A", meta: { encoder: "a", produced_by: "a" },
                                 intent: { purpose: "p", sender: "a", receiver: "s", note: bad } }), WireError);
  }
});
