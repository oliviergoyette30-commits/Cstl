// Arbres : memes relations, meme empreinte que le SDK Python (vecteurs produits
// par cstl_compact.tree_to_relations / tree_hash), et relecture identique.
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { treeToRelations, relationsToTree, treeHash, TreeError, type Json } from "../src/index.js";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(readFileSync(join(here, "..", "..", "test", "tree_vectors.json"), "utf8")) as
  { tree_id: string; data: Json; opts: { source?: string; nonce?: string }; relations: Record<string, string>[]; hash: string }[];

for (const v of vectors) {
  test(`arbre ${v.tree_id} : relations et empreinte identiques au Python`, () => {
    const rels = treeToRelations(v.data, v.tree_id, v.opts);
    assert.deepEqual(rels, v.relations);
    assert.equal(treeHash(rels, v.tree_id), v.hash);
    assert.deepEqual(relationsToTree(v.relations, v.tree_id), v.data);
  });
}

test("arbre incomplet ou nom invalide : refuse, jamais devine", () => {
  const v = vectors[0];
  const missing = v.relations.filter((r) => r.subject !== "o2.0");
  assert.throws(() => relationsToTree(missing, v.tree_id), TreeError);
  assert.throws(() => treeToRelations({ "bad name": [{ a: 1 }], "x y": 1 }, "t"), TreeError);
  assert.throws(() => treeToRelations({ a: 1 }, "bad id"), TreeError);
});
