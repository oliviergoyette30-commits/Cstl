// CSTL compact : memes textes et memes relations que le SDK Python
// (vecteurs produits par cstl_compact.encode / encode_symbols / decode / policy_to_*).
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { encode, encodeSymbols, decode, policyToCompact, policyToSymbols, CompactError, type Rel, type PolicyView } from "../src/index.js";

const here = dirname(fileURLToPath(import.meta.url));
const V = JSON.parse(readFileSync(join(here, "..", "..", "test", "compact_vectors.json"), "utf8")) as {
  cases: Record<string, unknown>[]; decode: { text: string; relations: Rel[] }[]; errors: { text: string }[];
  policy: PolicyView; policy_compact: string; policy_symbols: string;
};

test("encodage ligne, tableau, symboles : textes identiques au Python, relus identiques", () => {
  let n = 0;
  for (const c of V.cases) {
    const rels = c.relations as Rel[];
    for (const [name, f] of [["line", () => encode(rels)], ["table", () => encode(rels, { table: true })], ["symbols", () => encodeSymbols(rels)]] as const) {
      const want = c[name] as string | null;
      if (want === null) { assert.throws(f, CompactError); continue; }
      assert.equal(f(), want, `${name} : ${JSON.stringify(rels)}`);
      assert.deepEqual(decode(want), c[`${name}_decoded`]);
      n++;
    }
  }
  assert.ok(n > 800);
});

test("decodage de textes ecrits a la main, et refus de ce qui est illisible", () => {
  for (const d of V.decode) assert.deepEqual(decode(d.text), d.relations, d.text);
  for (const e of V.errors) assert.throws(() => decode(e.text), CompactError, e.text);
});

test("politique effective en compact et en symboles : identique au Python", () => {
  assert.equal(policyToCompact(V.policy), V.policy_compact);
  assert.equal(policyToSymbols(V.policy), V.policy_symbols);
});
